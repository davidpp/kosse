//! How the agent-facing commands (`list`, `url`, `port`, `profile`, `stop`, `force`) fail: one plain stderr line,
//! or with `--json` a `CommandError` record, and an exit code that tells a negative answer (1)
//! from a usage error (2: clap's, a directory outside any project, unreadable input), a refusal
//! (3) and not being able to observe (4).

use std::{
    io::{self, Write},
    process::ExitCode,
};

use kosse_core::{
    action::ActionResult,
    agent::{CommandError, CommandErrorCode, Confirmation, ProfileFieldProblem},
    profiles::StoreError,
    sockets::SocketsError,
};

use crate::text::printable;

#[derive(Debug)]
pub struct Failure {
    pub code: CommandErrorCode,
    pub message: String,
    /// Where a refused profile breaks a rule.
    pub problems: Vec<ProfileFieldProblem>,
    /// For a refused stop or force: the commands that would confirm it now.
    pub confirm: Vec<Confirmation>,
    /// For a stop or force the instance refused: its answer (boxed: it is large and rare).
    pub result: Option<Box<ActionResult>>,
}

impl Failure {
    pub fn new(code: CommandErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            problems: Vec::new(),
            confirm: Vec::new(),
            result: None,
        }
    }

    fn exit_code(&self) -> u8 {
        match self.code {
            CommandErrorCode::NotFound
            | CommandErrorCode::Ambiguous
            | CommandErrorCode::NoHttp
            | CommandErrorCode::TimedOut => 1,
            CommandErrorCode::NotInProject | CommandErrorCode::InputUnreadable => 2,
            CommandErrorCode::InvalidProfile
            | CommandErrorCode::NotProbeable
            | CommandErrorCode::RevisionConflict
            | CommandErrorCode::UnsafeRoot
            | CommandErrorCode::ConfirmationRequired
            | CommandErrorCode::Stale
            | CommandErrorCode::ForceNotOffered
            | CommandErrorCode::ActionRefused
            | CommandErrorCode::RateLimited => 3,
            CommandErrorCode::ProjectUnknown
            | CommandErrorCode::ScanFailed
            | CommandErrorCode::ConfigUnavailable
            | CommandErrorCode::ConfigWriteFailed
            | CommandErrorCode::OutputFailed
            | CommandErrorCode::NoInstance
            | CommandErrorCode::ActionFailed => 4,
        }
    }
}

impl From<SocketsError> for Failure {
    fn from(error: SocketsError) -> Self {
        Self::new(CommandErrorCode::ScanFailed, error.to_string())
    }
}

impl From<StoreError> for Failure {
    fn from(error: StoreError) -> Self {
        let message = error.to_string();
        match error {
            StoreError::Rejected(error) => Self {
                problems: vec![ProfileFieldProblem {
                    at: error.at,
                    problem: error.problem,
                }],
                ..Self::new(CommandErrorCode::InvalidProfile, message)
            },
            // The profile would make the file larger than Kossé reads.
            StoreError::TooLarge => Self::new(CommandErrorCode::InvalidProfile, message),
            StoreError::RevisionMismatch { .. } => {
                Self::new(CommandErrorCode::RevisionConflict, message)
            }
            StoreError::Io { .. } => Self::new(CommandErrorCode::ConfigWriteFailed, message),
            StoreError::Unusable { .. }
            | StoreError::NoHome
            | StoreError::RelativeDir
            | StoreError::Conflict(_) => Self::new(CommandErrorCode::ConfigUnavailable, message),
        }
    }
}

/// `<command>  (<process>, <project>)`; the command holds only numbers.
fn described(confirmation: &Confirmation) -> String {
    let process = confirmation.process.as_deref().unwrap_or("-");
    let project = confirmation.project.as_deref().unwrap_or("no project");
    printable(&format!("{}  ({process}, {project})", confirmation.command))
}

/// Writes a command's output; a closed pipe (`kosse list | head`) just means the reader has seen
/// enough.
pub fn written(result: io::Result<()>) -> Result<(), Failure> {
    match result.and_then(|()| io::stdout().lock().flush()) {
        Err(error) if error.kind() != io::ErrorKind::BrokenPipe => Err(Failure::new(
            CommandErrorCode::OutputFailed,
            format!("could not write the answer: {error}"),
        )),
        _ => Ok(()),
    }
}

/// The exit code of a command, reporting its failure on stderr.
pub fn exit(result: Result<(), Failure>, json: bool) -> ExitCode {
    let Err(failure) = result else {
        return ExitCode::SUCCESS;
    };
    failed(&failure, json)
}

/// Reports `failure` on stderr and returns its exit code.
pub fn failed(failure: &Failure, json: bool) -> ExitCode {
    if json {
        let record = CommandError {
            error: failure.code,
            message: failure.message.clone(),
            problems: failure.problems.clone(),
            confirm: failure.confirm.clone(),
            result: failure.result.as_deref().cloned(),
        };
        // Serializing a struct of an enum and a string cannot fail; stderr may be closed.
        if let Ok(line) = serde_json::to_string(&record) {
            report!("{line}");
        }
    } else {
        // Messages can quote paths and names: untrusted text.
        report!("kosse: {}", printable(&failure.message));
        for confirmation in &failure.confirm {
            report!("  {}", described(confirmation));
        }
    }
    ExitCode::from(failure.exit_code())
}
