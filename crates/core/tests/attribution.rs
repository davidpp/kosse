//! Project attribution for real owned fixture processes running inside temporary repositories.
//! Only fixture services are asserted on; the machine's other listeners are ignored.
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

use std::{
    fs,
    net::{IpAddr, Ipv4Addr},
    path::Path,
    time::{Duration, Instant},
};

use kosse_core::{
    attribution::{Attributor, HEAD_TTL, LAYOUT_TTL},
    model::{AttributionFailure, GitHead, Manifest, Observation, Project, ScanWarning, Service},
    scan::Scanner,
};
use kosse_fixtures::{
    Fixture, FixtureSpec,
    repo::{Sandbox, git, init_repo},
};

const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

fn sandbox() -> Sandbox {
    Sandbox::new().expect("sandbox")
}

fn run_in(dir: &Path) -> Fixture {
    Fixture::spawn(&FixtureSpec::new().listen(&[V4]).cwd(dir)).expect("spawn fixture")
}

fn service<'a>(observation: &'a Observation, fixture: &Fixture) -> &'a Service {
    observation
        .services
        .iter()
        .find(|service| service.process.identity.pid == fixture.pid())
        .unwrap_or_else(|| panic!("fixture {} missing", fixture.pid()))
}

fn project<'a>(observation: &'a Observation, fixture: &Fixture) -> &'a Project {
    let root = service(observation, fixture)
        .project
        .as_deref()
        .unwrap_or_else(|| panic!("fixture {} has no project", fixture.pid()));
    let mut matching = observation.projects.iter().filter(|p| p.root == root);
    let project = matching.next().expect("service project listed");
    assert!(matching.next().is_none(), "projects are listed once");
    project
}

fn text(path: &Path) -> String {
    path.to_str().expect("UTF-8 test path").to_owned()
}

fn branch(name: &str) -> GitHead {
    GitHead::Branch { name: name.into() }
}

#[test]
fn same_name_repositories_are_separate_projects() {
    let tree = sandbox();
    let first = tree.dir("one/app").expect("dir");
    let second = tree.dir("two/app").expect("dir");
    init_repo(&first).expect("repo");
    init_repo(&second).expect("repo");
    let (a, b) = (run_in(&first), run_in(&second));

    let observation = Scanner::new().scan().expect("scan");
    let (pa, pb) = (project(&observation, &a), project(&observation, &b));
    assert_eq!(pa.name, "app");
    assert_eq!(pb.name, "app");
    assert_eq!(pa.root, text(&first));
    assert_eq!(pb.root, text(&second));
    let (ga, gb) = (pa.git.as_ref().expect("git"), pb.git.as_ref().expect("git"));
    assert_ne!(ga.common_dir, gb.common_dir);
    assert_eq!(ga.common_dir, text(&first.join(".git")));
    assert_eq!(ga.repository_name, "app");
    assert!(!ga.linked);
    assert_eq!(ga.head, branch("main"));
}

#[test]
fn nested_packages_are_labelled_within_one_project() {
    let tree = sandbox();
    let mono = tree.dir("mono").expect("dir");
    init_repo(&mono).expect("repo");
    tree.file("mono/package.json", "{}").expect("file");
    tree.file("mono/packages/web/package.json", "{}")
        .expect("file");
    tree.file("mono/packages/web/Cargo.toml", "").expect("file");
    let web_src = tree.dir("mono/packages/web/src").expect("dir");
    let docs = tree.dir("mono/docs").expect("dir");
    let (web, root) = (run_in(&web_src), run_in(&docs));

    let observation = Scanner::new().scan().expect("scan");
    assert_eq!(project(&observation, &web).root, text(&mono));
    assert_eq!(project(&observation, &root).root, text(&mono));

    let web_package = service(&observation, &web)
        .package
        .clone()
        .expect("package");
    assert_eq!(web_package.path, "packages/web");
    assert_eq!(web_package.name, "web");
    assert_eq!(
        web_package.manifests,
        [Manifest::PackageJson, Manifest::CargoToml]
    );
    let root_package = service(&observation, &root)
        .package
        .clone()
        .expect("package");
    assert_eq!(root_package.path, "", "nearest manifest is the root one");
    assert_eq!(root_package.name, "mono");
}

#[test]
fn worktrees_of_one_repository_stay_separate_projects() {
    let tree = sandbox();
    let main = tree.dir("app").expect("dir");
    init_repo(&main).expect("repo");
    let linked = tree.root().join("app feature");
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            linked.to_str().expect("path"),
        ],
    )
    .expect("worktree add");
    let linked = linked.canonicalize().expect("canonical");
    assert!(
        linked.join(".git").is_file(),
        "a linked worktree has a .git file"
    );
    let (in_main, in_linked) = (run_in(&main), run_in(&linked));

    let observation = Scanner::new().scan().expect("scan");
    let (pm, pl) = (
        project(&observation, &in_main),
        project(&observation, &in_linked),
    );
    assert_ne!(pm.root, pl.root, "never merged by common Git directory");
    assert_eq!(pl.root, text(&linked));
    assert_eq!(pl.name, "app feature");
    let (gm, gl) = (pm.git.as_ref().expect("git"), pl.git.as_ref().expect("git"));
    assert_eq!(gm.common_dir, gl.common_dir, "one repository");
    assert_eq!(gl.repository_name, "app");
    assert!(!gm.linked);
    assert!(gl.linked);
    assert_eq!(gm.head, branch("main"));
    assert_eq!(gl.head, branch("feature"));
}

#[test]
fn gitfile_repositories_and_submodules_resolve_through_git() {
    let tree = sandbox();
    // `.git` is a file pointing at a separate Git directory.
    let separate = tree.dir("separate").expect("dir");
    let store = tree.dir("store").expect("dir").join("separate.git");
    git(
        &separate,
        &[
            "init",
            "-q",
            "--separate-git-dir",
            store.to_str().expect("path"),
        ],
    )
    .expect("init");
    git(
        &separate,
        &["commit", "-q", "--allow-empty", "-m", "initial"],
    )
    .expect("commit");
    assert!(separate.join(".git").is_file());

    // A submodule's `.git` is a file pointing into the superproject's modules directory.
    let upstream = tree.dir("upstream lib").expect("dir");
    init_repo(&upstream).expect("repo");
    let top = tree.dir("top").expect("dir");
    init_repo(&top).expect("repo");
    git(
        &top,
        &[
            "submodule",
            "add",
            "-q",
            upstream.to_str().expect("path"),
            "vendor/lib",
        ],
    )
    .expect("submodule add");
    let submodule = top.join("vendor/lib").canonicalize().expect("canonical");
    assert!(submodule.join(".git").is_file());

    let (in_separate, in_submodule) = (run_in(&separate), run_in(&submodule));
    let observation = Scanner::new().scan().expect("scan");

    let ps = project(&observation, &in_separate);
    assert_eq!(ps.root, text(&separate));
    let gs = ps.git.as_ref().expect("git");
    assert_eq!(
        gs.common_dir,
        text(&store.canonicalize().expect("canonical"))
    );
    assert_eq!(gs.repository_name, "separate");
    assert!(!gs.linked);
    assert_eq!(gs.head, branch("main"));

    let pm = project(&observation, &in_submodule);
    assert_eq!(
        pm.root,
        text(&submodule),
        "the submodule is its own project"
    );
    let gm = pm.git.as_ref().expect("git");
    assert_eq!(gm.repository_name, "lib");
    assert!(
        gm.common_dir.ends_with("/top/.git/modules/vendor/lib"),
        "{}",
        gm.common_dir
    );
    assert!(!gm.linked);
    // `submodule add` clones onto the upstream's branch; `submodule update` would detach.
    assert_eq!(gm.head, branch("main"));
}

#[test]
fn detached_head_reports_the_commit() {
    let tree = sandbox();
    let repo = tree.dir("detached").expect("dir");
    init_repo(&repo).expect("repo");
    git(&repo, &["checkout", "-q", "--detach"]).expect("detach");
    let commit = git(&repo, &["rev-parse", "--short", "HEAD"]).expect("rev-parse");
    let fixture = run_in(&repo);

    let observation = Scanner::new().scan().expect("scan");
    let head = &project(&observation, &fixture)
        .git
        .as_ref()
        .expect("git")
        .head;
    assert_eq!(head, &GitHead::Detached { commit });
}

#[test]
fn unicode_and_spaces_in_paths_keep_their_on_disk_form() {
    let tree = sandbox();
    // Decomposed "é" (e + U+0301) next to the sandbox's precomposed one.
    let repo = tree.dir("Cafe\u{301} 項目 dir").expect("dir");
    init_repo(&repo).expect("repo");
    let package_dir = repo.join("pkg ü");
    fs::create_dir(&package_dir).expect("dir");
    fs::write(package_dir.join("package.json"), "{}").expect("file");
    let fixture = run_in(&package_dir);

    let observation = Scanner::new().scan().expect("scan");
    let found = project(&observation, &fixture);
    assert_eq!(found.root, text(&repo));
    assert_eq!(
        found.name,
        repo.file_name().expect("name").to_string_lossy()
    );
    let package = service(&observation, &fixture)
        .package
        .clone()
        .expect("package");
    assert_eq!(package.name, "pkg ü");
}

#[test]
fn manifest_without_git_is_a_project_and_bare_directories_are_not() {
    let tree = sandbox();
    let loose = tree.dir("loose").expect("dir");
    tree.file("loose/pyproject.toml", "").expect("file");
    let loose_src = tree.dir("loose/src").expect("dir");
    let bare = tree.dir("nothing here").expect("dir");
    let (in_loose, in_bare) = (run_in(&loose_src), run_in(&bare));

    let observation = Scanner::new().scan().expect("scan");
    let found = project(&observation, &in_loose);
    assert_eq!(found.root, text(&loose));
    assert_eq!(found.git, None);
    let package = service(&observation, &in_loose)
        .package
        .clone()
        .expect("package");
    assert_eq!(
        (package.path.as_str(), package.manifests),
        ("", vec![Manifest::PyprojectToml])
    );

    let unattributed = service(&observation, &in_bare);
    assert_eq!(unattributed.project, None);
    assert_eq!(unattributed.package, None);
    assert!(
        !observation.warnings.iter().any(|warning| matches!(
            warning,
            ScanWarning::AttributionUnavailable { pid, .. } if *pid == in_bare.pid()
        )),
        "not being in a project is not a failure"
    );
}

#[test]
fn walks_stop_before_the_home_directory() {
    let tree = sandbox();
    let home = tree.dir("home").expect("dir");
    init_repo(&home).expect("dotfiles repository in home");
    tree.file("home/package.json", "{}").expect("file");
    let below = tree.dir("home/scratch").expect("dir");

    let mut attributor = Attributor::new(Some(&home));
    let (below, home) = (text(&below), text(&home));
    let results = attributor.attribute([below.as_str(), home.as_str()], Instant::now());
    for (cwd, result) in &results {
        assert_eq!(
            result,
            &Ok(None),
            "{cwd} must not belong to the home directory's repository"
        );
    }
    assert_eq!(attributor.git_commands(), 0);
}

#[test]
fn deleted_working_directory_is_a_warning_not_a_guess() {
    let tree = sandbox();
    let repo = tree.dir("doomed").expect("dir");
    init_repo(&repo).expect("repo");
    let inner = tree.dir("doomed/inner").expect("dir");
    let fixture = run_in(&inner);
    fs::remove_dir_all(&repo).expect("delete the fixture's directory");

    let observation = Scanner::new().scan().expect("scan");
    let found = service(&observation, &fixture);
    assert_eq!(found.project, None);
    assert!(
        observation
            .warnings
            .contains(&ScanWarning::AttributionUnavailable {
                pid: fixture.pid(),
                reason: AttributionFailure::CwdUnavailable,
            }),
        "{:?}",
        observation.warnings
    );
}

#[test]
fn broken_gitfile_is_a_cached_failure() {
    let tree = sandbox();
    let broken = tree.dir("broken").expect("dir");
    tree.file("broken/.git", "gitdir: /nonexistent/kosse\n")
        .expect("file");
    let cwd = text(&broken);

    let mut attributor = Attributor::new(None);
    let now = Instant::now();
    let first = attributor.attribute([cwd.as_str()], now);
    assert_eq!(first[cwd.as_str()], Err(AttributionFailure::GitFailed));
    let commands = attributor.git_commands();
    assert_eq!(commands, 1);
    attributor.attribute([cwd.as_str()], now + Duration::from_secs(1));
    assert_eq!(
        attributor.git_commands(),
        commands,
        "failure reused, no retry storm"
    );
}

#[test]
fn git_runs_once_per_worktree_and_only_again_after_the_cache_expires() {
    let tree = sandbox();
    let repo = tree.dir("cached").expect("dir");
    init_repo(&repo).expect("repo");
    tree.file("cached/a/package.json", "{}").expect("file");
    tree.file("cached/b/package.json", "{}").expect("file");
    let cwds = [repo.clone(), repo.join("a"), repo.join("b")].map(|dir| text(&dir));
    let cwds = cwds.iter().map(String::as_str);

    let mut attributor = Attributor::new(None);
    let now = Instant::now();
    attributor.attribute(cwds.clone(), now);
    // rev-parse for the layout plus symbolic-ref for the branch, shared by all three cwds.
    assert_eq!(attributor.git_commands(), 2);
    attributor.attribute(cwds.clone(), now + Duration::from_secs(1));
    assert_eq!(attributor.git_commands(), 2, "fresh cache: no git at all");

    attributor.attribute(cwds.clone(), now + HEAD_TTL);
    assert_eq!(attributor.git_commands(), 3, "only the branch is re-read");
    attributor.attribute(cwds.clone(), now + LAYOUT_TTL);
    assert_eq!(attributor.git_commands(), 5, "layout and branch re-read");

    // Directories no longer reported are forgotten.
    attributor.attribute(std::iter::empty(), now + LAYOUT_TTL);
    assert_eq!(attributor.cached(), (0, 0));
}

#[test]
fn branch_changes_are_seen_after_the_head_ttl() {
    let tree = sandbox();
    let repo = tree.dir("switching").expect("dir");
    init_repo(&repo).expect("repo");
    let cwd = text(&repo);
    let mut attributor = Attributor::new(None);
    let now = Instant::now();
    let head = |attributor: &mut Attributor, at| {
        attributor.attribute([cwd.as_str()], at)[cwd.as_str()]
            .clone()
            .expect("attributed")
            .expect("a project")
            .project
            .git
            .expect("git")
            .head
    };
    assert_eq!(head(&mut attributor, now), branch("main"));
    git(&repo, &["switch", "-q", "-c", "next"]).expect("switch");
    assert_eq!(
        head(&mut attributor, now + Duration::from_secs(1)),
        branch("main"),
        "cached"
    );
    assert_eq!(head(&mut attributor, now + HEAD_TTL), branch("next"));
}

#[test]
fn spent_git_budget_defers_new_worktrees_and_keeps_stale_ones() {
    let tree = sandbox();
    let known = tree.dir("known").expect("dir");
    let fresh = tree.dir("fresh").expect("dir");
    init_repo(&known).expect("repo");
    init_repo(&fresh).expect("repo");
    let (known, fresh) = (text(&known), text(&fresh));
    let mut attributor = Attributor::new(None);
    let now = Instant::now();
    let first = attributor.attribute([known.as_str()], now)[known.as_str()].clone();
    assert_eq!(attributor.git_commands(), 2);

    attributor.set_git_budget(Duration::ZERO);
    let later = now + LAYOUT_TTL;
    let results = attributor.attribute([known.as_str(), fresh.as_str()], later);
    assert_eq!(
        results[known.as_str()],
        first,
        "stale entry reused, not dropped"
    );
    assert_eq!(results[fresh.as_str()], Err(AttributionFailure::Deferred));
    assert_eq!(
        attributor.git_commands(),
        2,
        "no git once the budget is spent"
    );

    // Deferral is not cached: the next scan with budget resolves it.
    attributor.set_git_budget(kosse_core::attribution::GIT_BUDGET);
    let resolved = attributor.attribute([known.as_str(), fresh.as_str()], later);
    assert!(matches!(resolved[fresh.as_str()], Ok(Some(_))));
}
