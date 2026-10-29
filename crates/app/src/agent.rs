//! `kosse agent [skill [--write <dir> [--force]]]`: the guide for coding agents, printed as is or
//! as a `SKILL.md`. Both come from `agent.md`, so they cannot differ, and a test checks that every
//! command and flag the guide names exists.
//!
//! Kossé never installs the skill by itself (blueprint/agent-surface.md, D3). `--write <dir>` is
//! the one write: `<dir>/kosse/SKILL.md`, where `<dir>` must already be a real directory (Kossé
//! creates no agent configuration directory), and none of `<dir>`, `kosse` or `SKILL.md` may be a
//! symbolic link. A `SKILL.md` identical to this version's is left as is; one that differs (an
//! older version's, or edited by its owner) is replaced only with `--force`. Refusals exit 3 and
//! write nothing; a failed write exits 1.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use crate::{
    cli::{AgentArgs, AgentCommand, SkillArgs},
    text::printable,
};

const GUIDE: &str = include_str!("agent.md");

/// YAML frontmatter of the skill. The description is a plain scalar: no `: ` or ` #` in it.
const FRONTMATTER: &str = "---
name: kosse
description: Answers questions about local development servers on this Mac with the kosse CLI. Use when asked what is running for this project, which port or URL a dev server uses, whether a port is free and who holds it, to wait until a server is up or answers HTTP, to stop a server you started, or to customize how Kossé names this project's services. Prefer it over lsof, ps, curl retry loops and sleep.
---

";

const SKILL_DIR: &str = "kosse";
const SKILL_FILE: &str = "SKILL.md";
const REFUSED: u8 = 3;

fn skill() -> String {
    format!("{FRONTMATTER}{GUIDE}")
}

pub fn run(args: &AgentArgs) -> ExitCode {
    match &args.command {
        None => print(GUIDE),
        Some(AgentCommand::Skill(SkillArgs { write: None, .. })) => print(&skill()),
        Some(AgentCommand::Skill(SkillArgs {
            write: Some(dir),
            force,
        })) => match install(dir, *force) {
            Ok((written, path)) => print(&format!(
                "{}: {}\n",
                written.label(),
                printable(&path.display().to_string())
            )),
            Err(Refusal::Refused(message)) => {
                report!("kosse: {}; nothing was written", printable(&message));
                ExitCode::from(REFUSED)
            }
            Err(Refusal::Io(path, error)) => {
                report!(
                    "kosse: could not write {}: {error}",
                    printable(&path.display().to_string())
                );
                ExitCode::FAILURE
            }
        },
    }
}

fn print(text: &str) -> ExitCode {
    let mut stdout = io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Err(error) if error.kind() != io::ErrorKind::BrokenPipe => {
            report!("kosse: could not write the output: {error}");
            ExitCode::FAILURE
        }
        _ => ExitCode::SUCCESS,
    }
}

#[derive(Debug, Clone, Copy)]
enum Written {
    Created,
    Unchanged,
    Replaced,
}

impl Written {
    const fn label(self) -> &'static str {
        match self {
            Self::Created => "wrote",
            Self::Unchanged => "up to date",
            Self::Replaced => "replaced",
        }
    }
}

#[derive(Debug)]
enum Refusal {
    Refused(String),
    Io(PathBuf, io::Error),
}

fn io_at(path: &Path) -> impl FnOnce(io::Error) -> Refusal + '_ {
    move |error| Refusal::Io(path.to_owned(), error)
}

/// What is at `path`, without following a final symbolic link.
fn kind(path: &Path) -> Result<Option<fs::FileType>, Refusal> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata.file_type())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Refusal::Io(path.to_owned(), error)),
    }
}

fn real_directory(path: &Path) -> Result<bool, Refusal> {
    match kind(path)? {
        None => Ok(false),
        Some(kind) if kind.is_symlink() => Err(Refusal::Refused(format!(
            "{} is a symbolic link",
            path.display()
        ))),
        Some(kind) if kind.is_dir() => Ok(true),
        Some(_) => Err(Refusal::Refused(format!(
            "{} is not a directory",
            path.display()
        ))),
    }
}

fn install(dir: &Path, force: bool) -> Result<(Written, PathBuf), Refusal> {
    if !real_directory(dir)? {
        return Err(Refusal::Refused(format!(
            "{} does not exist; Kossé creates no agent configuration directory",
            dir.display()
        )));
    }
    let skill_dir = dir.join(SKILL_DIR);
    if !real_directory(&skill_dir)? {
        fs::create_dir(&skill_dir).map_err(io_at(&skill_dir))?;
    }
    let path = skill_dir.join(SKILL_FILE);
    let content = skill();
    match kind(&path)? {
        None => {
            // `create_new` refuses anything that appeared meanwhile, a symbolic link included.
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(io_at(&path))?;
            file.write_all(content.as_bytes()).map_err(io_at(&path))?;
            Ok((Written::Created, path))
        }
        Some(kind) if kind.is_symlink() => Err(Refusal::Refused(format!(
            "{} is a symbolic link",
            path.display()
        ))),
        Some(kind) if !kind.is_file() => Err(Refusal::Refused(format!(
            "{} is not a regular file",
            path.display()
        ))),
        Some(_) => {
            if fs::read(&path).map_err(io_at(&path))? == content.as_bytes() {
                return Ok((Written::Unchanged, path));
            }
            if !force {
                return Err(Refusal::Refused(format!(
                    "{} differs from this version's skill (older, or edited); pass --force to replace it",
                    path.display()
                )));
            }
            replace(&skill_dir, &path, &content)?;
            Ok((Written::Replaced, path))
        }
    }
}

/// Writes a sibling temporary file and renames it over `path`, which replaces the directory
/// entry itself and never writes through a link.
fn replace(skill_dir: &Path, path: &Path, content: &str) -> Result<(), Refusal> {
    let staged = skill_dir.join(format!(".{SKILL_FILE}.kosse-{}", std::process::id()));
    let result = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged)
        .and_then(|mut file| file.write_all(content.as_bytes()))
        .and_then(|()| fs::rename(&staged, path));
    if let Err(error) = result {
        let _ = fs::remove_file(&staged);
        return Err(Refusal::Io(path.to_owned(), error));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory as _;

    use super::{FRONTMATTER, GUIDE};
    use crate::cli::Cli;

    /// Every `kosse …` invocation in the guide's code spans.
    fn invocations() -> Vec<Vec<String>> {
        let spans = GUIDE.split('`').skip(1).step_by(2);
        let mut found = Vec::new();
        for span in spans {
            let mut rest = span;
            while let Some(at) = rest.find("kosse ") {
                let starts_word = rest[..at]
                    .chars()
                    .next_back()
                    .is_none_or(|before| !before.is_alphanumeric());
                rest = &rest[at + "kosse ".len()..];
                if !starts_word {
                    continue;
                }
                let end = rest.find([')', '|']).unwrap_or(rest.len());
                found.push(rest[..end].split_whitespace().map(str::to_owned).collect());
            }
        }
        found
    }

    /// The guide names only commands and flags that exist, so it cannot drift from the CLI.
    #[test]
    fn the_guide_names_only_real_commands_and_flags() {
        let mut root = Cli::command();
        root.build();
        let invocations = invocations();
        assert!(invocations.len() > 15, "found {invocations:?}");
        for words in invocations {
            let mut command = &root;
            let mut descending = true;
            for word in &words {
                if let Some(flag) = word.strip_prefix("--") {
                    descending = false;
                    let name = flag.split('=').next().unwrap_or(flag);
                    assert!(
                        command
                            .get_arguments()
                            .any(|arg| arg.get_long() == Some(name)),
                        "`kosse {}`: `{}` has no --{name}",
                        words.join(" "),
                        command.get_name()
                    );
                } else if descending && let Some(sub) = command.find_subcommand(word) {
                    command = sub;
                } else if descending && command.has_subcommands() {
                    // Placeholders like `<service>` are positional values, never subcommands.
                    assert!(
                        word.starts_with('<') || command.get_positionals().next().is_some(),
                        "`kosse {}`: `{}` has no subcommand {word}",
                        words.join(" "),
                        command.get_name()
                    );
                    descending = false;
                } else {
                    descending = false;
                }
            }
        }
    }

    /// The skill's frontmatter is what agent harnesses read to decide when to load it.
    #[test]
    fn the_skill_frontmatter_is_valid_plain_yaml() {
        let lines: Vec<&str> = FRONTMATTER.lines().collect();
        assert_eq!(lines.first(), Some(&"---"));
        assert_eq!(lines.get(1), Some(&"name: kosse"));
        let description = lines
            .get(2)
            .and_then(|line| line.strip_prefix("description: "))
            .unwrap_or_default();
        assert!(!description.is_empty() && description.len() <= 1024);
        assert!(!description.contains(": ") && !description.contains(" #"));
        assert_eq!(lines.get(3), Some(&"---"));
    }
}
