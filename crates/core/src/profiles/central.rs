//! Kossé's per-user configuration file: central project profiles keyed by canonical worktree
//! root, and the dashboard's preferences, in `~/Library/Application Support/Kosse/config.json`
//! (or `$KOSSE_CONFIG_DIR`).
//!
//! - Versioned JSON. A newer version, or a file that does not parse or validate, is reported and
//!   left exactly as it is: writes are refused until the user fixes or removes it, so the original
//!   stays recoverable.
//! - Fields this build does not know are kept on rewrite.
//! - Writes are atomic (temporary file in the same directory, `fsync`, `rename`, directory
//!   `fsync`) and serialized across processes by an advisory lock, so a dashboard save and a CLI
//!   command do not lose each other's change. The directory is created `0700`, the file `0600`.
//! - The action token and other secrets never go here.

use std::{
    collections::BTreeMap,
    fs::{self, DirBuilder, File, OpenOptions},
    io::{self, Write as _},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    contract::Preferences,
    profiles::{
        document::{ProjectProfile, unique_keys},
        source::{self, FileStamp, Links, MAX_FILE_BYTES},
        validate::{self, ProfileError},
        view::ProfileProblem,
    },
};

/// Overrides the configuration directory (tests, sandboxes). Must be absolute.
pub const CONFIG_DIR_ENV: &str = "KOSSE_CONFIG_DIR";
pub const CONFIG_FILE_NAME: &str = "config.json";
const LOCK_FILE_NAME: &str = "config.lock";
pub const CONFIG_VERSION: u32 = 1;

/// The configuration file's contents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CentralConfig {
    pub version: u32,
    /// Profiles by canonical absolute worktree root. Never matched by name: a moved project needs
    /// [`CentralStore::reassociate`].
    #[serde(
        default,
        deserialize_with = "unique_keys",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub projects: BTreeMap<String, ProjectProfile>,
    /// Part of config version 1: a file without it has the defaults, and a build that predates
    /// it keeps it as an unknown field.
    #[serde(default, skip_serializing_if = "StoredPreferences::is_empty")]
    pub preferences: StoredPreferences,
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

/// [`Preferences`] as stored: a missing preference has its default, and preferences this build
/// does not know are kept on rewrite. A known one with the wrong type makes the file invalid.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredPreferences {
    #[serde(default)]
    pub all_listeners: bool,
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

impl StoredPreferences {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    #[must_use]
    pub fn known(&self) -> Preferences {
        Preferences {
            all_listeners: self.all_listeners,
        }
    }
}

impl CentralConfig {
    #[must_use]
    pub fn new() -> Self {
        Self {
            version: CONFIG_VERSION,
            projects: BTreeMap::new(),
            preferences: StoredPreferences::default(),
            unknown: BTreeMap::new(),
        }
    }

    /// Every rule, for every project.
    fn validate(&self) -> Result<(), ProfileError> {
        if self.version != CONFIG_VERSION {
            return Err(ProfileError {
                at: "version".into(),
                problem: format!("must be {CONFIG_VERSION}"),
            });
        }
        for (root, profile) in &self.projects {
            let at = format!("projects[{root:?}]");
            if !Path::new(root).is_absolute() {
                return Err(ProfileError {
                    at,
                    problem: "keys are absolute worktree roots".into(),
                });
            }
            validate::profile(profile).map_err(|error| ProfileError {
                at: if error.at.is_empty() {
                    at.clone()
                } else {
                    format!("{at}.{}", error.at)
                },
                problem: error.problem,
            })?;
        }
        Ok(())
    }
}

impl Default for CentralConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The nested profiles' versions only, read leniently so a profile written by a newer Kossé is
/// reported as newer even when the rest of its shape is unknown.
#[derive(Deserialize)]
struct NestedVersions {
    projects: BTreeMap<String, crate::profiles::document::VersionProbe>,
}

/// Parses and validates the file's contents.
pub fn parse(bytes: &[u8]) -> Result<CentralConfig, ProfileProblem> {
    let probe: crate::profiles::document::VersionProbe =
        serde_json::from_slice(bytes).map_err(|error| ProfileProblem::Invalid {
            reason: error.to_string(),
        })?;
    source::check_version(probe.version.as_ref())?;
    if let Ok(nested) = serde_json::from_slice::<NestedVersions>(bytes) {
        for profile in nested.projects.values() {
            if let Err(newer @ ProfileProblem::NewerVersion { .. }) =
                source::check_version(profile.version.as_ref())
            {
                return Err(newer);
            }
        }
    }
    let config: CentralConfig = source::parse_versioned(bytes)?;
    config.validate().map_err(|error| ProfileProblem::Invalid {
        reason: error.to_string(),
    })?;
    Ok(config)
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("no home directory to keep the configuration in; set {CONFIG_DIR_ENV}")]
    NoHome,
    #[error("{CONFIG_DIR_ENV} must be an absolute path")]
    RelativeDir,
    #[error(
        "{}: {problem}; fix or remove it (Kossé will not overwrite it)",
        path.display()
    )]
    Unusable {
        path: PathBuf,
        problem: ProfileProblem,
    },
    #[error("profile refused: {0}")]
    Rejected(ProfileError),
    #[error("the configuration would exceed {MAX_FILE_BYTES} bytes")]
    TooLarge,
    #[error("{0}")]
    Conflict(String),
    #[error("could not update {}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
}

/// The configuration directory and its file.
#[derive(Debug, Clone)]
pub struct CentralStore {
    dir: PathBuf,
}

impl CentralStore {
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// `$KOSSE_CONFIG_DIR`, else the user's application support directory.
    pub fn from_env() -> Result<Self, StoreError> {
        if let Some(dir) = std::env::var_os(CONFIG_DIR_ENV) {
            let dir = PathBuf::from(dir);
            if !dir.is_absolute() {
                return Err(StoreError::RelativeDir);
            }
            return Ok(Self::new(dir));
        }
        let home = std::env::home_dir().ok_or(StoreError::NoHome)?;
        Ok(Self::new(home.join("Library/Application Support/Kosse")))
    }

    #[must_use]
    pub fn file(&self) -> PathBuf {
        self.dir.join(CONFIG_FILE_NAME)
    }

    /// The file's current contents; `Ok(None)` when there is none yet.
    pub fn load(&self) -> Result<Option<(CentralConfig, FileStamp)>, ProfileProblem> {
        let Some((bytes, stamp)) = source::read(&self.file(), Links::Follow)? else {
            return Ok(None);
        };
        Ok(Some((parse(&bytes)?, stamp)))
    }

    /// Sets (`Some`) or removes (`None`) the profile of `root`, keeping fields this build does
    /// not know from the profile it replaces. Returns the profile as stored.
    pub fn save(
        &self,
        root: &str,
        profile: Option<ProjectProfile>,
    ) -> Result<Option<ProjectProfile>, StoreError> {
        if !Path::new(root).is_absolute() {
            return Err(StoreError::Conflict(
                "a project root is an absolute path".into(),
            ));
        }
        // Profiles are matched by canonical root; a root that no longer exists is kept as given
        // so its profile can still be removed.
        let canonical = fs::canonicalize(root)
            .ok()
            .and_then(|path| path.to_str().map(str::to_owned));
        let root = canonical.as_deref().unwrap_or(root);
        self.update(|config| {
            let Some(mut profile) = profile else {
                config.projects.remove(root);
                return Ok(None);
            };
            validate::profile(&profile).map_err(StoreError::Rejected)?;
            if let Some(previous) = config.projects.get(root) {
                profile.keep_unknown_from(previous);
            }
            config.projects.insert(root.to_owned(), profile.clone());
            Ok(Some(profile))
        })
    }

    /// The saved preferences, or the defaults when there is no file yet. A file that cannot be
    /// used is reported, as for a save.
    pub fn preferences(&self) -> Result<Preferences, StoreError> {
        match self.load() {
            Ok(config) => Ok(config
                .map(|(config, _)| config.preferences.known())
                .unwrap_or_default()),
            Err(problem) => Err(StoreError::Unusable {
                path: self.file(),
                problem,
            }),
        }
    }

    /// Replaces the preferences this build knows, keeping the others. Returns them as stored.
    pub fn save_preferences(&self, preferences: Preferences) -> Result<Preferences, StoreError> {
        self.update(|config| {
            config.preferences.all_listeners = preferences.all_listeners;
            Ok(config.preferences.known())
        })
    }

    /// Moves the profile saved for `from` to the directory `to` (a moved or re-cloned project).
    /// Never done implicitly. Returns the canonical new root.
    pub fn reassociate(&self, from: &str, to: &Path) -> Result<String, StoreError> {
        let to = fs::canonicalize(to)
            .ok()
            .filter(|path| path.is_dir())
            .and_then(|path| path.to_str().map(str::to_owned))
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "{} is not an existing directory with a UTF-8 path",
                    to.display()
                ))
            })?;
        self.update(|config| {
            if !config.projects.contains_key(from) {
                return Err(StoreError::Conflict(format!(
                    "no profile is saved for {from}"
                )));
            }
            if config.projects.contains_key(&to) {
                return Err(StoreError::Conflict(format!(
                    "{to} already has a profile; remove it first"
                )));
            }
            if let Some(profile) = config.projects.remove(from) {
                config.projects.insert(to.clone(), profile);
            }
            Ok(to.clone())
        })
    }

    /// The file to replace: the configuration file, or the file it links to. A link to nowhere is
    /// refused rather than replaced by a regular file.
    fn write_target(&self) -> Result<PathBuf, StoreError> {
        let file = self.file();
        match fs::symlink_metadata(&file) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                fs::canonicalize(&file).map_err(|source| StoreError::Io {
                    path: file.clone(),
                    source: io::Error::new(
                        source.kind(),
                        format!("it is a symbolic link whose target cannot be resolved ({source})"),
                    ),
                })
            }
            _ => Ok(file),
        }
    }

    /// Read, change, validate, write atomically, under the lock.
    fn update<T>(
        &self,
        change: impl FnOnce(&mut CentralConfig) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let io_error = |source| StoreError::Io {
            path: self.file(),
            source,
        };
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)
            .map_err(io_error)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(self.dir.join(LOCK_FILE_NAME))
            .map_err(io_error)?;
        lock.lock().map_err(io_error)?;

        let mut config = match self.load() {
            Ok(Some((config, _))) => config,
            Ok(None) => CentralConfig::new(),
            Err(problem) => {
                return Err(StoreError::Unusable {
                    path: self.file(),
                    problem,
                });
            }
        };
        let result = change(&mut config)?;
        config.validate().map_err(StoreError::Rejected)?;
        let mut text = serde_json::to_vec_pretty(&config)
            .map_err(|error| io_error(io::Error::other(error)))?;
        text.push(b'\n');
        if text.len() > MAX_FILE_BYTES as usize {
            return Err(StoreError::TooLarge);
        }

        // A symbolic link (to a dotfiles checkout, say) stays a link: the target is replaced, in
        // its own directory so that the rename is atomic. tempfile creates the file 0600.
        let target = self.write_target()?;
        let target_dir = target.parent().unwrap_or(&self.dir).to_path_buf();
        let mut temporary = tempfile::Builder::new()
            .prefix(".config-")
            .suffix(".tmp")
            .tempfile_in(&target_dir)
            .map_err(io_error)?;
        temporary.write_all(&text).map_err(io_error)?;
        temporary.as_file().sync_all().map_err(io_error)?;
        temporary
            .persist(&target)
            .map_err(|error| io_error(error.error))?;
        File::open(&target_dir)
            .and_then(|dir| dir.sync_all())
            .map_err(io_error)?;
        drop(lock);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn store() -> (tempfile::TempDir, CentralStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = CentralStore::new(dir.path().join("Kosse"));
        (dir, store)
    }

    fn profile(name: &str) -> ProjectProfile {
        ProjectProfile {
            name: Some(name.into()),
            ..ProjectProfile::new()
        }
    }

    #[test]
    fn saves_atomically_with_private_permissions() {
        let (_dir, store) = store();
        store.save("/work/app", Some(profile("App"))).expect("save");
        let mode = |path: &Path| fs::metadata(path).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode(&store.dir), 0o700);
        assert_eq!(mode(&store.file()), 0o600);
        let (config, _) = store.load().expect("load").expect("exists");
        assert_eq!(config.projects["/work/app"].name.as_deref(), Some("App"));
        let leftovers: Vec<_> = fs::read_dir(&store.dir)
            .expect("list")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(".config-"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");

        store.save("/work/app", None).expect("remove");
        let (config, _) = store.load().expect("load").expect("exists");
        assert!(config.projects.is_empty());
    }

    #[test]
    fn unknown_fields_are_kept_on_rewrite() {
        let (_dir, store) = store();
        fs::create_dir_all(&store.dir).expect("dir");
        let original = r#"{"version":1,"preferences":{"theme":"dark"},"projects":{"/work/app":{"$schema":"./kosse.schema.json","version":1,"name":"Old","later":true}}}"#;
        fs::write(store.file(), original).expect("write");
        store.save("/work/app", Some(profile("New"))).expect("save");
        let written: Value =
            serde_json::from_slice(&fs::read(store.file()).expect("read")).expect("json");
        assert_eq!(written["preferences"]["theme"], "dark");
        assert_eq!(written["projects"]["/work/app"]["later"], true);
        assert_eq!(
            written["projects"]["/work/app"]["$schema"],
            "./kosse.schema.json"
        );
        assert_eq!(written["projects"]["/work/app"]["name"], "New");
    }

    #[test]
    fn preferences_default_persist_and_keep_what_this_build_does_not_know() {
        let (_dir, store) = store();
        assert_eq!(
            store.preferences().expect("defaults"),
            Preferences::default()
        );
        assert!(!store.file().exists(), "reading creates nothing");

        store.save("/work/app", Some(profile("App"))).expect("save");
        let written: Value =
            serde_json::from_slice(&fs::read(store.file()).expect("read")).expect("json");
        assert!(
            written.get("preferences").is_none(),
            "defaults are not written"
        );

        fs::write(
            store.file(),
            r#"{"version":1,"preferences":{"theme":"dark"},"projects":{"/work/app":{"version":1,"name":"App"}}}"#,
        )
        .expect("write");
        let on = Preferences {
            all_listeners: true,
        };
        assert_eq!(store.save_preferences(on).expect("save"), on);
        assert_eq!(store.preferences().expect("load"), on);
        let written: Value =
            serde_json::from_slice(&fs::read(store.file()).expect("read")).expect("json");
        assert_eq!(written["preferences"]["allListeners"], true);
        assert_eq!(written["preferences"]["theme"], "dark");
        assert_eq!(written["projects"]["/work/app"]["name"], "App");

        fs::write(
            store.file(),
            r#"{"version":1,"preferences":{"allListeners":"yes"}}"#,
        )
        .expect("write");
        assert!(matches!(
            store.preferences(),
            Err(StoreError::Unusable { .. })
        ));
        assert!(matches!(
            store.save_preferences(on),
            Err(StoreError::Unusable { .. })
        ));
    }

    #[test]
    fn corrupt_and_newer_files_are_never_overwritten() {
        for (contents, newer) in [
            (&b"{\"version\":1,\"projects\":{\"/a\":"[..], false),
            (
                br#"{"version":1,"projects":{"/a":{"version":1},"/a":{"version":1}}}"#,
                false,
            ),
            (
                br#"{"version":1,"projects":{"relative":{"version":1}}}"#,
                false,
            ),
            (br#"{"version":7,"projects":[]}"#, true),
            (br#"{"version":0}"#, false),
            (
                br#"{"version":1,"projects":{"/a":{"version":2,"services":"later"}}}"#,
                true,
            ),
        ] {
            let (_dir, store) = store();
            fs::create_dir_all(&store.dir).expect("dir");
            fs::write(store.file(), contents).expect("write");
            let refused = store.save("/work/app", Some(profile("App")));
            match refused {
                Err(StoreError::Unusable { problem, .. }) => assert_eq!(
                    matches!(problem, ProfileProblem::NewerVersion { .. }),
                    newer,
                    "{problem}"
                ),
                other => panic!("expected a refusal, got {other:?}"),
            }
            assert!(matches!(
                store.save_preferences(Preferences::default()),
                Err(StoreError::Unusable { .. })
            ));
            assert_eq!(fs::read(store.file()).expect("read"), contents);
        }
    }

    #[test]
    fn a_symbolic_link_stays_a_link_and_its_target_is_replaced() {
        let (dir, store) = store();
        fs::create_dir_all(&store.dir).expect("dir");
        let dotfiles = dir.path().join("dotfiles");
        fs::create_dir(&dotfiles).expect("dir");
        let target = dotfiles.join("kosse.json");
        fs::write(&target, r#"{"version":1}"#).expect("write");
        std::os::unix::fs::symlink(&target, store.file()).expect("link");
        store.save("/work/app", Some(profile("App"))).expect("save");
        assert!(
            fs::symlink_metadata(store.file())
                .expect("stat")
                .file_type()
                .is_symlink()
        );
        let written: Value =
            serde_json::from_slice(&fs::read(&target).expect("read")).expect("json");
        assert_eq!(written["projects"]["/work/app"]["name"], "App");

        fs::remove_file(&target).expect("dangle");
        assert!(matches!(
            store.save("/work/app", None),
            Err(StoreError::Io { .. })
        ));
        assert!(!target.exists(), "a dangling link is not replaced");
    }

    #[test]
    fn roots_are_saved_canonically() {
        let (dir, store) = store();
        let real = dir.path().join("real");
        fs::create_dir(&real).expect("dir");
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(&real, &alias).expect("link");
        store
            .save(alias.to_str().expect("utf-8"), Some(profile("App")))
            .expect("save");
        let (config, _) = store.load().expect("load").expect("exists");
        let canonical = real.canonicalize().expect("canonical");
        assert!(
            config
                .projects
                .contains_key(canonical.to_str().expect("utf-8"))
        );
    }

    #[test]
    fn invalid_profiles_are_rejected_before_writing() {
        let (_dir, store) = store();
        let mut bad = ProjectProfile::new();
        bad.services.insert(
            "web".into(),
            crate::profiles::document::ServiceProfile {
                probe_path: Some("//evil.example/".into()),
                ..Default::default()
            },
        );
        assert!(matches!(
            store.save("/work/app", Some(bad)),
            Err(StoreError::Rejected(_))
        ));
        assert!(!store.file().exists());
    }

    #[test]
    fn reassociation_is_explicit_and_refuses_overwrites() {
        let (dir, store) = store();
        let moved = dir.path().join("moved");
        fs::create_dir(&moved).expect("dir");
        store.save("/old/app", Some(profile("App"))).expect("save");
        let new_root = store.reassociate("/old/app", &moved).expect("move");
        assert_eq!(
            new_root,
            moved
                .canonicalize()
                .expect("canonical")
                .to_str()
                .expect("utf-8")
        );
        let (config, _) = store.load().expect("load").expect("exists");
        assert!(!config.projects.contains_key("/old/app"));
        assert_eq!(config.projects[&new_root].name.as_deref(), Some("App"));

        store
            .save("/old/app", Some(profile("Other")))
            .expect("save");
        assert!(matches!(
            store.reassociate("/old/app", &moved),
            Err(StoreError::Conflict(_))
        ));
        assert!(matches!(
            store.reassociate("/nowhere", &moved),
            Err(StoreError::Conflict(_))
        ));
    }
}
