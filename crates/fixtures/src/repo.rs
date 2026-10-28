//! Disposable directory trees and Git repositories for attribution tests.
//!
//! Test repositories are built with the same `git` Stackscan uses (the active developer
//! directory's, from `xcode-select -p`), isolated from the user's configuration (no global or
//! system config, so no signing, hooks or templates apply) and from any inherited `GIT_*`
//! variables (a test run from a hook or `rebase -x` must not touch the enclosing repository),
//! with a fixed identity.

use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use tempfile::TempDir;

/// Spaces and non-ASCII on purpose: attribution must survive both.
const SANDBOX_PREFIX: &str = "stackscan repos é-";

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(
        "no git in the active developer directory (`xcode-select -p`); install the Command Line Tools"
    )]
    NoGit,
    #[error("git {args:?} in {dir} failed: {stderr}")]
    Git {
        dir: PathBuf,
        args: Vec<String>,
        stderr: String,
    },
}

/// A temporary directory tree, removed on drop.
pub struct Sandbox {
    root: PathBuf,
    _dir: TempDir,
}

impl Sandbox {
    pub fn new() -> Result<Self, RepoError> {
        let dir = tempfile::Builder::new().prefix(SANDBOX_PREFIX).tempdir()?;
        let root = dir.path().canonicalize()?;
        Ok(Self { root, _dir: dir })
    }

    /// Canonical root of the tree.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Creates `relative` (and its parents) and returns its canonical path.
    pub fn dir(&self, relative: &str) -> Result<PathBuf, RepoError> {
        let path = self.root.join(relative);
        fs::create_dir_all(&path)?;
        Ok(path.canonicalize()?)
    }

    /// Writes a file (creating parent directories).
    pub fn file(&self, relative: &str, contents: &str) -> Result<PathBuf, RepoError> {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, contents)?;
        Ok(path)
    }
}

/// `git` from the active developer directory, as `stackscan_core`'s attribution runs it.
fn developer_git() -> Result<PathBuf, RepoError> {
    let output = Command::new("/usr/bin/xcode-select").arg("-p").output()?;
    let developer_dir = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let git = Path::new(&developer_dir).join("usr/bin/git");
    if output.status.success() && !developer_dir.is_empty() && git.is_file() {
        Ok(git)
    } else {
        Err(RepoError::NoGit)
    }
}

/// Runs isolated `git` in `dir` and returns its trimmed stdout.
pub fn git(dir: &Path, args: &[&str]) -> Result<String, RepoError> {
    let mut command = Command::new(developer_git()?);
    for (key, _) in env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    let output = command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "user.name=Stackscan Tests",
            "-c",
            "user.email=tests@stackscan.invalid",
            "-c",
            "init.defaultBranch=main",
            // Local paths as submodule URLs.
            "-c",
            "protocol.file.allow=always",
        ])
        .args(args)
        .current_dir(dir)
        .output()?;
    if !output.status.success() {
        return Err(RepoError::Git {
            dir: dir.to_path_buf(),
            args: args.iter().map(|&arg| arg.to_owned()).collect(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// `git init` plus one empty commit, so branches, worktrees and detaching all work.
pub fn init_repo(dir: &Path) -> Result<(), RepoError> {
    fs::create_dir_all(dir)?;
    git(dir, &["init", "-q"])?;
    git(dir, &["commit", "-q", "--allow-empty", "-m", "initial"])?;
    Ok(())
}
