//! `kosse agent` and `kosse agent skill` through the compiled binary, in temporary directories.
#![expect(
    clippy::expect_used,
    reason = "test helpers fail the test by panicking"
)]

use std::{
    fs,
    os::unix::fs::symlink,
    path::Path,
    process::{Command, Output},
};

use kosse_fixtures::repo::Sandbox;

fn kosse(cwd: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("HOME", home)
        .env("KOSSE_CONFIG_DIR", "/var/empty/kosse-tests")
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run kosse")
}

const NONE: [&str; 0] = [];

fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// D3: printing is the default and writes nothing; the skill is the guide plus frontmatter.
#[test]
fn printing_the_guide_or_skill_writes_nothing() {
    let sandbox = Sandbox::new().expect("sandbox");
    let cwd = sandbox.dir("cwd").expect("cwd");
    let home = sandbox.dir("home").expect("home");

    let guide = kosse(&cwd, &home, &["agent"]);
    let skill = kosse(&cwd, &home, &["agent", "skill"]);
    assert!(guide.status.success() && skill.status.success());
    let guide = String::from_utf8(guide.stdout).expect("UTF-8");
    let skill = String::from_utf8(skill.stdout).expect("UTF-8");
    assert!(guide.starts_with("# Kossé for coding agents\n"), "{guide}");
    assert!(
        skill.starts_with("---\nname: kosse\ndescription: "),
        "{skill}"
    );
    assert!(skill.ends_with(&format!("---\n\n{guide}")), "{skill}");

    assert_eq!(entries(&cwd), NONE);
    assert_eq!(entries(&home), NONE);
    assert_eq!(entries(sandbox.root()), ["cwd", "home"]);
}

/// `--write` writes only `<dir>/kosse/SKILL.md`, the printed skill; an identical file is left
/// alone, a different one (edited by its owner) only with `--force`.
#[test]
fn write_installs_the_skill_and_keeps_a_changed_file_without_force() {
    let sandbox = Sandbox::new().expect("sandbox");
    let home = sandbox.dir("home").expect("home");
    let skills = sandbox.dir("skills").expect("skills");
    let target = skills.join("kosse").join("SKILL.md");
    let dir = skills.to_str().expect("UTF-8");
    let printed = kosse(&home, &home, &["agent", "skill"]).stdout;

    let wrote = kosse(&home, &home, &["agent", "skill", "--write", dir]);
    assert!(wrote.status.success(), "{wrote:?}");
    assert_eq!(fs::read(&target).expect("SKILL.md"), printed);
    assert_eq!(entries(&skills), ["kosse"]);
    assert_eq!(entries(&skills.join("kosse")), ["SKILL.md"]);

    let again = kosse(&home, &home, &["agent", "skill", "--write", dir]);
    assert!(again.status.success(), "{again:?}");
    assert!(String::from_utf8_lossy(&again.stdout).starts_with("up to date: "));

    fs::write(&target, "my own notes\n").expect("edit");
    let kept = kosse(&home, &home, &["agent", "skill", "--write", dir]);
    assert_eq!(kept.status.code(), Some(3), "{kept:?}");
    assert!(String::from_utf8_lossy(&kept.stderr).contains("--force"));
    assert_eq!(
        fs::read_to_string(&target).expect("SKILL.md"),
        "my own notes\n"
    );

    let forced = kosse(&home, &home, &["agent", "skill", "--write", dir, "--force"]);
    assert!(forced.status.success(), "{forced:?}");
    assert_eq!(fs::read(&target).expect("SKILL.md"), printed);
    assert_eq!(
        entries(&skills.join("kosse")),
        ["SKILL.md"],
        "no staged file left"
    );
    assert_eq!(entries(&home), NONE);
}

/// `--write` never follows a symbolic link or creates the agent's directory, `--force` included:
/// each refusal exits 3 and leaves the link's target untouched.
#[test]
fn write_refuses_links_and_missing_or_non_directory_targets() {
    let sandbox = Sandbox::new().expect("sandbox");
    let home = sandbox.dir("home").expect("home");
    let elsewhere = sandbox.dir("elsewhere").expect("elsewhere");
    let run = |dir: &Path| {
        kosse(
            &home,
            &home,
            &[
                "agent",
                "skill",
                "--write",
                dir.to_str().expect("UTF-8"),
                "--force",
            ],
        )
    };
    let refused = |output: &Output| {
        assert_eq!(output.status.code(), Some(3), "{output:?}");
        assert_eq!(output.stdout, b"");
    };

    // The directory itself: missing, a file, a link to a directory.
    refused(&run(&sandbox.root().join("missing")));
    assert!(!sandbox.root().join("missing").exists());
    let file = sandbox.root().join("file");
    fs::write(&file, "x").expect("file");
    refused(&run(&file));
    let linked = sandbox.root().join("linked");
    symlink(&elsewhere, &linked).expect("symlink");
    refused(&run(&linked));
    assert_eq!(entries(&elsewhere), NONE);

    // `<dir>/kosse` as a link to another directory.
    let skills = sandbox.dir("skills").expect("skills");
    symlink(&elsewhere, skills.join("kosse")).expect("symlink");
    refused(&run(&skills));
    assert_eq!(entries(&elsewhere), NONE);

    // `<dir>/kosse/SKILL.md` as a link to someone else's file.
    let other = sandbox.dir("other").expect("other");
    let precious = elsewhere.join("precious.md");
    fs::write(&precious, "keep me\n").expect("precious");
    fs::create_dir(other.join("kosse")).expect("kosse dir");
    symlink(&precious, other.join("kosse").join("SKILL.md")).expect("symlink");
    refused(&run(&other));
    assert_eq!(
        fs::read_to_string(&precious).expect("precious"),
        "keep me\n"
    );
    assert!(
        fs::symlink_metadata(other.join("kosse").join("SKILL.md"))
            .expect("link")
            .file_type()
            .is_symlink()
    );
}
