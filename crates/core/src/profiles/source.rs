//! Reading profile files: size-bounded, regular files only, change detection by metadata, and
//! version-first parsing (a newer format is reported as newer, not as malformed).

use std::{
    fs::{self, OpenOptions},
    io::{self, Read},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};

use serde::de::DeserializeOwned;

use crate::profiles::{
    document::{PROFILE_VERSION, ProjectProfile, VersionProbe},
    validate,
    view::ProfileProblem,
};

/// Largest profile file read, central or repository (blueprint/project-metadata.md).
pub const MAX_FILE_BYTES: u32 = 64 * 1024;

/// Identifies one version of a file: a rewrite (in place or by rename) changes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    device: u64,
    inode: u64,
    len: u64,
    modified_ns: i128,
}

impl FileStamp {
    fn of(metadata: &fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            len: metadata.len(),
            modified_ns: i128::from(metadata.mtime()) * 1_000_000_000
                + i128::from(metadata.mtime_nsec()),
        }
    }
}

/// How symbolic links are treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Links {
    /// The user's own configuration may be a link (to a dotfiles checkout, say).
    Follow,
    /// A cloned repository is untrusted: its file must not point elsewhere on disk.
    Refuse,
}

/// The current stamp of `path`; `Ok(None)` when it does not exist.
pub fn stamp(path: &Path, links: Links) -> Result<Option<FileStamp>, ProfileProblem> {
    let metadata = match links {
        Links::Follow => fs::metadata(path),
        Links::Refuse => fs::symlink_metadata(path),
    };
    match metadata {
        Ok(metadata) if metadata.is_file() => Ok(Some(FileStamp::of(&metadata))),
        Ok(_) => Err(ProfileProblem::NotAFile),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(unreadable(&error)),
    }
}

/// Reads at most [`MAX_FILE_BYTES`]. `Ok(None)` when the file does not exist. The opened file must
/// be the regular file that was checked (a swap between check and open is refused).
pub fn read(path: &Path, links: Links) -> Result<Option<(Vec<u8>, FileStamp)>, ProfileProblem> {
    let Some(checked) = stamp(path, links)? else {
        return Ok(None);
    };
    if checked.len > u64::from(MAX_FILE_BYTES) {
        return Err(too_large());
    }
    // Never follow a link swapped in after the check (repository files), and never block on a
    // FIFO or device: the scanner thread reads these.
    let mut flags = libc::O_NONBLOCK;
    if links == Links::Refuse {
        flags |= libc::O_NOFOLLOW;
    }
    let file = match OpenOptions::new().read(true).custom_flags(flags).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        // ELOOP: the final component became a symbolic link.
        Err(error) if error.raw_os_error() == Some(libc::ELOOP) => {
            return Err(ProfileProblem::NotAFile);
        }
        Err(error) => return Err(unreadable(&error)),
    };
    let opened = file.metadata().map_err(|error| unreadable(&error))?;
    if !opened.is_file() || opened.ino() != checked.inode || opened.dev() != checked.device {
        return Err(ProfileProblem::NotAFile);
    }
    let mut bytes = Vec::new();
    file.take(u64::from(MAX_FILE_BYTES) + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| unreadable(&error))?;
    if bytes.len() > MAX_FILE_BYTES as usize {
        return Err(too_large());
    }
    Ok(Some((bytes, FileStamp::of(&opened))))
}

fn too_large() -> ProfileProblem {
    ProfileProblem::TooLarge {
        limit_bytes: MAX_FILE_BYTES,
    }
}

fn unreadable(error: &io::Error) -> ProfileProblem {
    ProfileProblem::Unreadable {
        reason: error.kind().to_string(),
    }
}

/// Parses a versioned document: the version first, then the whole.
pub fn parse_versioned<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, ProfileProblem> {
    let probe: VersionProbe = serde_json::from_slice(bytes).map_err(|error| invalid(&error))?;
    check_version(probe.version.as_ref())?;
    serde_json::from_slice(bytes).map_err(|error| invalid(&error))
}

/// `version` must be a whole number from 1; above this build's version it is newer, not invalid.
pub(crate) fn check_version(version: Option<&serde_json::Value>) -> Result<(), ProfileProblem> {
    let Some(version) = version else {
        return Err(ProfileProblem::Invalid {
            reason: "missing `version`".into(),
        });
    };
    match version.as_u64() {
        Some(number) if number > u64::from(PROFILE_VERSION) => Err(ProfileProblem::NewerVersion {
            version: number.to_string(),
        }),
        Some(1..) => Ok(()),
        _ => Err(ProfileProblem::Invalid {
            reason: format!("`version` must be a whole number from 1, not {version}"),
        }),
    }
}

/// A repository profile file's contents, fully validated.
pub fn parse_profile(bytes: &[u8]) -> Result<ProjectProfile, ProfileProblem> {
    let profile: ProjectProfile = parse_versioned(bytes)?;
    validate::profile(&profile).map_err(|error| ProfileProblem::Invalid {
        reason: error.to_string(),
    })?;
    Ok(profile)
}

fn invalid(error: &serde_json::Error) -> ProfileProblem {
    ProfileProblem::Invalid {
        reason: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_versions_are_reported_as_newer_not_malformed() {
        let newer = br#"{"version": 2, "services": "a shape this build cannot read"}"#;
        assert_eq!(
            parse_profile(newer),
            Err(ProfileProblem::NewerVersion {
                version: "2".into()
            })
        );
        assert!(matches!(
            parse_profile(br#"{"name": "x"}"#),
            Err(ProfileProblem::Invalid { .. })
        ));
        assert!(matches!(
            parse_profile(br#"{"version": 0}"#),
            Err(ProfileProblem::Invalid { .. })
        ));
        assert!(matches!(
            parse_profile(br#"{"version": -1}"#),
            Err(ProfileProblem::Invalid { .. })
        ));
        assert!(matches!(
            parse_profile(br#"{"version": "1"}"#),
            Err(ProfileProblem::Invalid { .. })
        ));
        assert!(matches!(
            parse_profile(b"{\"version\": 1,"),
            Err(ProfileProblem::Invalid { .. })
        ));
        assert!(parse_profile(br#"{"version": 1}"#).is_ok());
    }
}
