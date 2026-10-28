//! `kosse config schema|path|move`.

use std::io::{self, Write};

use kosse_core::profiles::{CentralStore, StoreError};

use crate::cli::ConfigCommand;

/// The profile schema as committed. `kosse_core::profiles::json_schema` generates it; a test
/// fails when the two differ. Embedding the text keeps the schema generator out of the binary.
const SCHEMA: &str = include_str!("../../../docs/kosse.schema.json");

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("could not write the output: {0}")]
    Write(#[source] io::Error),
}

pub fn run(command: &ConfigCommand) -> Result<(), ConfigError> {
    let mut stdout = io::stdout().lock();
    let written = match command {
        ConfigCommand::Schema => stdout.write_all(SCHEMA.as_bytes()),
        ConfigCommand::Path => writeln!(stdout, "{}", CentralStore::from_env()?.file().display()),
        ConfigCommand::Move { from, to } => {
            let root = CentralStore::from_env()?.reassociate(from, to)?;
            writeln!(stdout, "moved the profile of {from} to {root}")
        }
    };
    match written.and_then(|()| stdout.flush()) {
        Err(error) if error.kind() != io::ErrorKind::BrokenPipe => Err(ConfigError::Write(error)),
        _ => Ok(()),
    }
}
