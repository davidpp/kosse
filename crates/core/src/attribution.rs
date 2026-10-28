//! Which project and package a working directory belongs to.
//!
//! One upward walk from the canonical working directory, stopping before the user's home
//! directory, the filesystem root, or a different filesystem, finds the nearest package
//! manifest and the nearest `.git` entry (directory or file). A `.git` entry makes that
//! directory a candidate worktree root, which `git` itself then confirms and describes. Outside
//! Git, the nearest manifest directory is the project. Nothing found means no project: an
//! unknown or unresolvable directory is never guessed.
//!
//! Results are cached: walks per reported working directory and Git locations per worktree for
//! [`LAYOUT_TTL`], branches for [`HEAD_TTL`]. Each scan drops entries it no longer needs, so the
//! cache is bounded by the live set. Failures are cached too, so a broken repository costs one
//! `git` call per TTL rather than one per scan.
//!
//! Each scan may start `git` commands for at most [`GIT_BUDGET`] (each command is itself bounded
//! by [`crate::git::GIT_TIMEOUT`]). Once it is spent, stale entries are reused as they are and
//! unresolved worktrees are reported as `deferred` until a later scan; deferrals are not cached.
//! The stat-only walk has no deadline of its own: a working directory on a stalled network mount
//! blocks the scan (readers see the snapshot age), which is preferred to leaking a blocked thread
//! per stalled path.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::{
    git::Git,
    model::{AttributionFailure, GitHead, GitWorktree, Manifest, PackageLabel, Project},
};

/// How long a directory layout (manifests, `.git` location, common directory) is trusted.
pub const LAYOUT_TTL: Duration = Duration::from_secs(60);
/// How long a branch or detached commit is trusted. Checkouts are frequent, layouts are not.
pub const HEAD_TTL: Duration = Duration::from_secs(15);
/// Time per scan during which new `git` commands may start.
pub const GIT_BUDGET: Duration = Duration::from_secs(1);

/// Where a working directory sits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribution {
    pub project: Project,
    pub package: Option<PackageLabel>,
}

/// What the stat-only walk found for one working directory.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Place {
    /// Candidate worktree root (has a `.git` entry), or the package directory outside Git.
    root: PathBuf,
    in_git: bool,
    package: Option<(PathBuf, Vec<Manifest>)>,
}

#[derive(Debug, Clone)]
struct Cached<T> {
    value: T,
    at: Instant,
}

impl<T> Cached<T> {
    fn fresh(&self, now: Instant, ttl: Duration) -> bool {
        now.saturating_duration_since(self.at) < ttl
    }
}

/// Git facts for one worktree root.
#[derive(Debug, Clone)]
struct Worktree {
    location: Cached<Result<Location, AttributionFailure>>,
    head: Option<Cached<GitHead>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Location {
    common_dir: PathBuf,
    linked: bool,
}

enum GitState {
    NotLocated,
    /// Looked for and absent; looked for again after [`LAYOUT_TTL`] (tools may get installed).
    Missing {
        checked_at: Instant,
    },
    Found(Git),
}

/// Resolves and caches attribution for the scanner. One per [`crate::scan::Scanner`].
pub struct Attributor {
    /// Canonical home directory: the walk never considers it or anything above it.
    home: Option<PathBuf>,
    git: GitState,
    git_budget: Duration,
    /// End of the current scan's git budget (wall-clock, independent of the `now` passed in).
    budget_ends: Instant,
    places: HashMap<String, Cached<Result<Option<Place>, AttributionFailure>>>,
    worktrees: HashMap<PathBuf, Worktree>,
}

impl Attributor {
    /// `home` bounds every walk; `None` walks up to the filesystem root or boundary.
    #[must_use]
    pub fn new(home: Option<&Path>) -> Self {
        Self {
            home: home.and_then(|home| fs::canonicalize(home).ok()),
            git: GitState::NotLocated,
            git_budget: GIT_BUDGET,
            budget_ends: Instant::now(),
            places: HashMap::new(),
            worktrees: HashMap::new(),
        }
    }

    /// Bounded by the current user's home directory.
    #[must_use]
    pub fn for_current_user() -> Self {
        Self::new(std::env::home_dir().as_deref())
    }

    /// `git` commands run so far.
    #[must_use]
    pub fn git_commands(&self) -> u64 {
        match &self.git {
            GitState::Found(git) => git.commands(),
            GitState::NotLocated | GitState::Missing { .. } => 0,
        }
    }

    /// Replaces [`GIT_BUDGET`] for later scans.
    pub fn set_git_budget(&mut self, budget: Duration) {
        self.git_budget = budget;
    }

    /// Cached entries (working directories, worktrees), for bounding checks.
    #[must_use]
    pub fn cached(&self) -> (usize, usize) {
        (self.places.len(), self.worktrees.len())
    }

    /// Attributes each reported working directory, reusing fresh cache entries, then forgets
    /// every entry these directories did not use.
    pub fn attribute<'a>(
        &mut self,
        cwds: impl IntoIterator<Item = &'a str>,
        now: Instant,
    ) -> BTreeMap<&'a str, Result<Option<Attribution>, AttributionFailure>> {
        self.budget_ends = Instant::now() + self.git_budget;
        let mut results = BTreeMap::new();
        for cwd in cwds {
            if results.contains_key(cwd) {
                continue;
            }
            let attribution = self.attribute_one(cwd, now);
            results.insert(cwd, attribution);
        }
        self.places
            .retain(|cwd, _| results.contains_key(cwd.as_str()));
        // Failed locations stay cached as well: only worktrees no current directory uses go.
        let used: HashSet<&Path> = self
            .places
            .values()
            .filter_map(|cached| match &cached.value {
                Ok(Some(place)) if place.in_git => Some(place.root.as_path()),
                _ => None,
            })
            .collect();
        self.worktrees
            .retain(|root, _| used.contains(root.as_path()));
        results
    }

    fn attribute_one(
        &mut self,
        cwd: &str,
        now: Instant,
    ) -> Result<Option<Attribution>, AttributionFailure> {
        let place = match self.places.get(cwd) {
            Some(cached) if cached.fresh(now, LAYOUT_TTL) => cached.value.clone(),
            _ => {
                let place = walk(Path::new(cwd), self.home.as_deref());
                self.places.insert(
                    cwd.to_owned(),
                    Cached {
                        value: place.clone(),
                        at: now,
                    },
                );
                place
            }
        };
        let Some(place) = place? else {
            return Ok(None);
        };
        let package = place
            .package
            .as_ref()
            .map(|(dir, manifests)| package_label(&place.root, dir, manifests));
        let git = if place.in_git {
            Some(self.worktree(&place.root, now)?)
        } else {
            None
        };
        Ok(Some(Attribution {
            project: Project {
                root: lossy(&place.root),
                name: name_of(&place.root),
                git,
            },
            package,
        }))
    }

    fn worktree(&mut self, root: &Path, now: Instant) -> Result<GitWorktree, AttributionFailure> {
        let cached = self
            .worktrees
            .get(root)
            .map(|worktree| worktree.location.clone());
        let location = match cached {
            Some(location) if location.fresh(now, LAYOUT_TTL) => location.value,
            stale => match self.with_git(now, |git| locate(git, root)) {
                // Out of budget: keep what is known, resolve the rest on a later scan.
                Err(AttributionFailure::Deferred) => match stale {
                    Some(location) => location.value,
                    None => return Err(AttributionFailure::Deferred),
                },
                result => {
                    let location = Cached {
                        value: result.clone(),
                        at: now,
                    };
                    self.worktrees
                        .entry(root.to_path_buf())
                        .and_modify(|worktree| worktree.location = location.clone())
                        .or_insert(Worktree {
                            location,
                            head: None,
                        });
                    result
                }
            },
        }?;

        let cached = self
            .worktrees
            .get(root)
            .and_then(|worktree| worktree.head.clone());
        let head = match cached {
            Some(head) if head.fresh(now, HEAD_TTL) => head.value,
            stale => match self.with_git(now, |git| Ok(head_of(git, root))) {
                Ok(head) => {
                    if let Some(worktree) = self.worktrees.get_mut(root) {
                        worktree.head = Some(Cached {
                            value: head.clone(),
                            at: now,
                        });
                    }
                    head
                }
                // Deferred or unavailable: the last known head, if any, is still the best answer.
                Err(_) => stale.map_or(GitHead::Unknown, |head| head.value),
            },
        };

        Ok(GitWorktree {
            common_dir: lossy(&location.common_dir),
            repository_name: repository_name(&location.common_dir),
            linked: location.linked,
            head,
        })
    }

    fn with_git<T>(
        &mut self,
        now: Instant,
        run: impl FnOnce(&mut Git) -> Result<T, AttributionFailure>,
    ) -> Result<T, AttributionFailure> {
        if Instant::now() >= self.budget_ends {
            return Err(AttributionFailure::Deferred);
        }
        let look = match self.git {
            GitState::NotLocated => true,
            GitState::Missing { checked_at } => {
                now.saturating_duration_since(checked_at) >= LAYOUT_TTL
            }
            GitState::Found(_) => false,
        };
        if look {
            self.git = Git::locate().map_or(GitState::Missing { checked_at: now }, GitState::Found);
        }
        match &mut self.git {
            GitState::Found(git) => run(git),
            GitState::NotLocated | GitState::Missing { .. } => {
                Err(AttributionFailure::GitUnavailable)
            }
        }
    }
}

/// The stat-only upward walk described in the module docs.
fn walk(cwd: &Path, home: Option<&Path>) -> Result<Option<Place>, AttributionFailure> {
    let cwd = fs::canonicalize(cwd).map_err(|_| AttributionFailure::CwdUnavailable)?;
    let device = fs::metadata(&cwd)
        .map_err(|_| AttributionFailure::CwdUnavailable)?
        .dev();
    let mut package = None;
    for dir in cwd.ancestors() {
        let at_boundary = Some(dir) == home
            || dir.parent().is_none()
            || fs::metadata(dir).map_or(true, |metadata| metadata.dev() != device);
        if at_boundary {
            break;
        }
        if package.is_none() {
            let manifests: Vec<Manifest> = Manifest::ALL
                .into_iter()
                .filter(|manifest| dir.join(manifest.file_name()).is_file())
                .collect();
            if !manifests.is_empty() {
                package = Some((dir.to_path_buf(), manifests));
            }
        }
        // Directory or file alike; git decides whether it is valid.
        if fs::symlink_metadata(dir.join(".git")).is_ok() {
            return Ok(Some(Place {
                root: dir.to_path_buf(),
                in_git: true,
                package,
            }));
        }
    }
    Ok(package.map(|(dir, manifests)| Place {
        root: dir.clone(),
        in_git: false,
        package: Some((dir, manifests)),
    }))
}

/// Asks git for the worktree root, common directory and per-worktree Git directory of `root`,
/// and checks that git agrees `root` is the worktree root.
fn locate(git: &mut Git, root: &Path) -> Result<Location, AttributionFailure> {
    let output = git.run(
        root,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-common-dir",
            "--git-dir",
        ],
    )?;
    if !output.success {
        return Err(AttributionFailure::GitFailed);
    }
    // One path per line; a path containing a newline yields extra lines and is refused.
    let lines: Vec<&str> = output.stdout.lines().collect();
    let [toplevel, common_dir, git_dir] = lines[..] else {
        return Err(AttributionFailure::GitFailed);
    };
    let canonical = |path: &str| fs::canonicalize(path).map_err(|_| AttributionFailure::GitFailed);
    if canonical(toplevel)? != root {
        return Err(AttributionFailure::GitFailed);
    }
    let common_dir = canonical(common_dir)?;
    let linked = canonical(git_dir)? != common_dir;
    Ok(Location { common_dir, linked })
}

/// Branch (also an unborn one), else the detached commit, else unknown.
fn head_of(git: &mut Git, root: &Path) -> GitHead {
    match git.run(root, &["symbolic-ref", "-q", "HEAD"]) {
        Ok(output) if output.success => {
            let reference = output.stdout.trim_end_matches('\n');
            let name = reference.strip_prefix("refs/heads/").unwrap_or(reference);
            if name.is_empty() {
                GitHead::Unknown
            } else {
                GitHead::Branch {
                    name: name.to_owned(),
                }
            }
        }
        // Exit 1 without output: HEAD is not a symbolic ref, i.e. detached.
        Ok(output) if output.code == Some(1) => {
            match git.run(root, &["rev-parse", "-q", "--verify", "--short", "HEAD"]) {
                Ok(output) if output.success && !output.stdout.trim().is_empty() => {
                    GitHead::Detached {
                        commit: output.stdout.trim().to_owned(),
                    }
                }
                _ => GitHead::Unknown,
            }
        }
        _ => GitHead::Unknown,
    }
}

fn package_label(root: &Path, dir: &Path, manifests: &[Manifest]) -> PackageLabel {
    let relative = dir.strip_prefix(root).unwrap_or(Path::new(""));
    let path = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    PackageLabel {
        path,
        name: name_of(dir),
        manifests: manifests.to_vec(),
    }
}

/// `…/app/.git` → `app`; `…/app.git` → `app`; `…/.git/modules/sub` → `sub`.
fn repository_name(common_dir: &Path) -> String {
    let base = if common_dir.file_name().is_some_and(|name| name == ".git") {
        common_dir.parent().unwrap_or(common_dir)
    } else {
        common_dir
    };
    let name = name_of(base);
    name.strip_suffix(".git")
        .filter(|stem| !stem.is_empty())
        .map_or(name.clone(), str::to_owned)
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| lossy(path), |name| name.to_string_lossy().into_owned())
}

fn lossy(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_names_come_from_the_common_directory() {
        assert_eq!(repository_name(Path::new("/src/app/.git")), "app");
        assert_eq!(repository_name(Path::new("/store/app.git")), "app");
        assert_eq!(
            repository_name(Path::new("/src/top/.git/modules/sub")),
            "sub"
        );
    }

    #[test]
    fn package_paths_are_relative_and_slash_separated() {
        let label = package_label(
            Path::new("/r/mono"),
            Path::new("/r/mono/packages/web"),
            &[Manifest::PackageJson],
        );
        assert_eq!(label.path, "packages/web");
        assert_eq!(label.name, "web");
        let root = package_label(Path::new("/r/mono"), Path::new("/r/mono"), &[]);
        assert_eq!(root.path, "");
        assert_eq!(root.name, "mono");
    }
}
