//! The scanner's view of profile files: reloaded when their metadata changes (checked at most once
//! per [`CHECK_INTERVAL`] per file), the last good version kept when an update is invalid, and
//! forgotten when a file is deleted.

use std::{
    collections::{BTreeSet, HashMap},
    path::Path,
    time::{Duration, Instant},
};

use crate::{
    model::{Observation, ScanWarning},
    profiles::{
        central::{self, CentralConfig, CentralStore},
        document::{ProjectProfile, REPO_FILE_NAME},
        effective,
        source::{self, FileStamp, Links},
        view::{ProfileProblem, ProfileSource},
    },
};

/// Least time between two checks of one file.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(1);
/// How long a repository profile stays known after its project was last observed, so its
/// services show as configured, not observed, rather than vanish with the last process.
pub const REPOSITORY_RETENTION: Duration = Duration::from_secs(60);

/// One file's last good contents and current problem.
struct Watched<T> {
    stamp: Option<FileStamp>,
    checked_at: Option<Instant>,
    good: Option<T>,
    /// Counts changes of `good`, so derived state is recomputed only when it changes.
    generation: u64,
    problem: Option<ProfileProblem>,
}

impl<T> Watched<T> {
    fn new() -> Self {
        Self {
            stamp: None,
            checked_at: None,
            good: None,
            generation: 0,
            problem: None,
        }
    }

    fn refresh(
        &mut self,
        path: &Path,
        links: Links,
        now: Instant,
        parse: impl Fn(&[u8]) -> Result<T, ProfileProblem>,
    ) {
        if self
            .checked_at
            .is_some_and(|at| now.saturating_duration_since(at) < CHECK_INTERVAL)
        {
            return;
        }
        self.checked_at = Some(now);
        let current = match source::stamp(path, links) {
            Ok(Some(stamp)) if Some(stamp) == self.stamp => return,
            Ok(Some(_)) => source::read(path, links),
            Ok(None) => Ok(None),
            Err(problem) => Err(problem),
        };
        match current {
            // Deleted: the user removed the profile, which is not a problem.
            Ok(None) => {
                self.stamp = None;
                self.good = None;
                self.generation += 1;
                self.problem = None;
            }
            Ok(Some((bytes, stamp))) => {
                self.stamp = Some(stamp);
                match parse(&bytes) {
                    Ok(value) => {
                        self.good = Some(value);
                        self.generation += 1;
                        self.problem = None;
                    }
                    Err(problem) => self.problem = Some(problem),
                }
            }
            Err(problem) => {
                self.stamp = None;
                self.problem = Some(problem);
            }
        }
    }

    fn warning(&self, source: ProfileSource, path: &Path) -> Option<ScanWarning> {
        self.problem
            .as_ref()
            .map(|problem| ScanWarning::ProfileUnusable {
                source,
                path: path.to_string_lossy().into_owned(),
                problem: problem.clone(),
                last_good_retained: self.good.is_some(),
            })
    }
}

struct Repository {
    file: Watched<ProjectProfile>,
    last_seen: Instant,
}

/// Central and repository profiles as the scanner applies them. One per scanner.
pub struct Profiles {
    store: CentralStore,
    central: Watched<CentralConfig>,
    /// Hand-edited central keys that are not canonical and so can never match, for the central
    /// file version (`generation`) in `noncanonical_of`.
    noncanonical: Vec<ScanWarning>,
    noncanonical_of: Option<u64>,
    repositories: HashMap<String, Repository>,
}

impl Profiles {
    #[must_use]
    pub fn new(store: CentralStore) -> Self {
        Self {
            store,
            central: Watched::new(),
            noncanonical: Vec::new(),
            noncanonical_of: None,
            repositories: HashMap::new(),
        }
    }

    /// Reloads changed files, then adds effective metadata to `observation`: `profiles`, each
    /// service's `profile`, declared HTTP candidates and warnings. `own_user` is Kossé's effective
    /// user (see [`crate::scan::Scanner::own_user`]).
    pub fn apply(&mut self, observation: &mut Observation, own_user: Option<u32>) {
        let now = Instant::now();
        let central_file = self.store.file();
        self.central
            .refresh(&central_file, Links::Follow, now, central::parse);

        let observed: BTreeSet<String> = observation
            .projects
            .iter()
            .map(|project| project.root.clone())
            .collect();
        for root in &observed {
            let repository = self
                .repositories
                .entry(root.clone())
                .or_insert_with(|| Repository {
                    file: Watched::new(),
                    last_seen: now,
                });
            repository.last_seen = now;
            repository.file.refresh(
                &Path::new(root).join(REPO_FILE_NAME),
                Links::Refuse,
                now,
                source::parse_profile,
            );
        }
        self.repositories.retain(|_, repository| {
            now.saturating_duration_since(repository.last_seen) < REPOSITORY_RETENTION
        });

        if self.noncanonical_of != Some(self.central.generation) {
            self.noncanonical_of = Some(self.central.generation);
            self.noncanonical = noncanonical_roots(self.central.good.as_ref());
        }
        let mut warnings: Vec<ScanWarning> = self
            .central
            .warning(ProfileSource::Central, &central_file)
            .into_iter()
            .chain(self.noncanonical.iter().cloned())
            .collect();
        let mut repository_roots: Vec<&String> = self.repositories.keys().collect();
        repository_roots.sort();
        for root in repository_roots {
            let path = Path::new(root).join(REPO_FILE_NAME);
            warnings.extend(
                self.repositories[root]
                    .file
                    .warning(ProfileSource::Repository, &path),
            );
        }

        let central = self.central.good.as_ref().map(|config| &config.projects);
        let repository = |root: &str| {
            self.repositories
                .get(root)
                .and_then(|repository| repository.file.good.as_ref())
        };
        let with_repository = self
            .repositories
            .iter()
            .filter(|(_, repository)| repository.file.good.is_some())
            .map(|(root, _)| root);
        let mut metadata: Vec<_> = effective::roots(central, with_repository)
            .into_iter()
            .map(|root| {
                effective::merge(
                    root,
                    central.and_then(|projects| projects.get(root)),
                    repository(root),
                    observed.contains(root),
                )
            })
            .collect();
        warnings.extend(effective::assign(observation, &mut metadata, own_user));
        observation.profiles = metadata;
        observation.warnings.extend(warnings);
    }
}

/// Central keys naming an existing directory by a non-canonical path (a symbolic link, `/tmp` for
/// `/private/tmp`): they can never match, since projects are keyed by canonical root.
fn noncanonical_roots(config: Option<&CentralConfig>) -> Vec<ScanWarning> {
    config
        .into_iter()
        .flat_map(|config| config.projects.keys())
        .filter_map(|root| {
            let canonical = std::fs::canonicalize(root).ok()?;
            let canonical = canonical.to_string_lossy();
            (canonical != root.as_str()).then(|| ScanWarning::ProfileRootNotCanonical {
                root: root.clone(),
                canonical: canonical.into_owned(),
            })
        })
        .collect()
}
