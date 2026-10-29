//! How the agent-facing read commands (`list`, `url`, `port`) fail: one plain stderr line, or
//! with `--json` a `CommandError` record, and an exit code that tells a negative answer (1) from
//! a refusal (3) and from not being able to observe (4). Usage errors are clap's (2).

use std::{
    io::{self, Write},
    process::ExitCode,
};

use kosse_core::{
    agent::{CommandError, CommandErrorCode},
    profiles::StoreError,
    sockets::SocketsError,
};

use crate::text::printable;

#[derive(Debug)]
pub struct Failure {
    pub code: CommandErrorCode,
    pub message: String,
}

impl Failure {
    pub fn new(code: CommandErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn exit_code(&self) -> u8 {
        match self.code {
            CommandErrorCode::NotFound | CommandErrorCode::Ambiguous | CommandErrorCode::NoHttp => {
                1
            }
            CommandErrorCode::NotInProject => 3,
            CommandErrorCode::ProjectUnknown
            | CommandErrorCode::ScanFailed
            | CommandErrorCode::ConfigUnavailable
            | CommandErrorCode::OutputFailed => 4,
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
        Self::new(CommandErrorCode::ConfigUnavailable, error.to_string())
    }
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

/// The exit code of a read command, reporting its failure on stderr.
pub fn exit(result: Result<(), Failure>, json: bool) -> ExitCode {
    let Err(failure) = result else {
        return ExitCode::SUCCESS;
    };
    if json {
        let record = CommandError {
            error: failure.code,
            message: failure.message.clone(),
        };
        // Serializing a struct of an enum and a string cannot fail; stderr may be closed.
        if let Ok(line) = serde_json::to_string(&record) {
            report!("{line}");
        }
    } else {
        // Messages can quote paths and names: untrusted text.
        report!("kosse: {}", printable(&failure.message));
    }
    ExitCode::from(failure.exit_code())
}
