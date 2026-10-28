//! `stackscan list`: one scan, printed as a table or as the JSON snapshot contract.
//!
//! There is no resident instance to ask yet, so this always runs its own one-shot scan (revision 1,
//! no interval).

use std::{
    collections::HashMap,
    fmt::Write as _,
    io::{self, Write},
};

use stackscan_core::{
    model::{GitHead, Observation, Project, Service, Snapshot},
    scan::Scanner,
    snapshot::{AttemptTimer, SnapshotTracker},
    sockets::SocketsError,
};

use crate::cli::ListArgs;

#[derive(Debug, thiserror::Error)]
pub enum ListError {
    #[error(transparent)]
    Scan(#[from] SocketsError),
    #[error("could not write the listing: {0}")]
    Write(#[source] io::Error),
}

pub fn run(args: &ListArgs) -> Result<(), ListError> {
    let timer = AttemptTimer::start();
    // CPU needs two observations of a process, so a one-shot scan reports it as warming up.
    let observation = Scanner::new().scan()?;
    let snapshot = SnapshotTracker::new(None).succeeded(observation, timer.finish());
    let observation = snapshot.observation.as_ref();
    let mut stdout = io::stdout().lock();
    let written = if args.json {
        write_json(&mut stdout, &snapshot)
    } else {
        // A successful scan always yields an observation.
        observation.map_or(Ok(()), |observation| write_table(&mut stdout, observation))
    };
    match written.and_then(|()| stdout.flush()) {
        // A closed pipe (`stackscan list | head`) just means the reader has seen enough.
        Err(error) if error.kind() != io::ErrorKind::BrokenPipe => {
            return Err(ListError::Write(error));
        }
        _ => {}
    }
    if !args.json {
        for warning in observation
            .iter()
            .flat_map(|observation| &observation.warnings)
        {
            report!("stackscan: warning: {warning}");
        }
    }
    Ok(())
}

fn write_json(out: &mut impl Write, snapshot: &Snapshot) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *out, snapshot)?;
    out.write_all(b"\n")
}

fn write_table(out: &mut impl Write, observation: &Observation) -> io::Result<()> {
    let projects: HashMap<&str, &Project> = observation
        .projects
        .iter()
        .map(|project| (project.root.as_str(), project))
        .collect();
    writeln!(
        out,
        "{:<8} {:<24} {:<6} {:<32} {:<20} ADDRESSES",
        "PID", "PROCESS", "PORT", "PROJECT", "BRANCH"
    )?;
    for service in &observation.services {
        let name = service
            .process
            .name
            .as_deref()
            .map_or_else(|| "?".to_owned(), printable);
        let project = service
            .project
            .as_deref()
            .and_then(|root| projects.get(root).copied());
        let (project_label, branch) = labels(service, project);
        for endpoint in &service.endpoints {
            writeln!(
                out,
                "{:<8} {:<24} {:<6} {:<32} {:<20} {}",
                service.process.identity.pid,
                name,
                endpoint.port,
                project_label,
                branch,
                endpoint.bind_addresses.join(", ")
            )?;
        }
    }
    Ok(())
}

/// `project/package/path` and the branch (or `detached@commit`); `-` when unknown.
fn labels(service: &Service, project: Option<&Project>) -> (String, String) {
    let Some(project) = project else {
        return ("-".to_owned(), "-".to_owned());
    };
    let mut label = project.name.clone();
    if let Some(package) = service
        .package
        .as_ref()
        .filter(|package| !package.path.is_empty())
    {
        label.push('/');
        label.push_str(&package.path);
    }
    let branch = match project.git.as_ref().map(|git| &git.head) {
        Some(GitHead::Branch { name }) => printable(name),
        Some(GitHead::Detached { commit }) => format!("detached@{}", printable(commit)),
        Some(GitHead::Unknown) => "?".to_owned(),
        None => "-".to_owned(),
    };
    (printable(&label), branch)
}

/// Process names are untrusted: escape characters that can drive or disguise terminal output.
fn printable(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_control() || is_invisible_format(character) {
            let _ = write!(out, "{}", character.escape_unicode());
        } else {
            out.push(character);
        }
    }
    out
}

/// Format characters that reorder or hide text: bidi embeddings, overrides and isolates
/// (Trojan Source), zero-width characters, line/paragraph separators and the BOM. std has no
/// Unicode general-category lookup, so the relevant ranges are listed.
fn is_invisible_format(character: char) -> bool {
    matches!(
        character,
        '\u{061C}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::printable;

    #[test]
    fn control_and_invisible_format_characters_are_escaped() {
        assert_eq!(printable("node\u{1b}[2Jx\n"), "node\\u{1b}[2Jx\\u{a}");
        assert_eq!(
            printable("a\u{202E}b\u{2066}c\u{200B}d"),
            "a\\u{202e}b\\u{2066}c\\u{200b}d"
        );
        assert_eq!(printable("vite é"), "vite é");
    }
}
