//! Sharing rendered refs: `fetch`, `push`, and what plain Git must not do.

mod common;

use common::{Repo, World, scrub_git_env, tpl};

/// Give a world a bare remote and publish `main` to it.
fn with_remote(world: &World) -> Repo {
    let remote_path = world.dir.path().join("remote.git");
    let mut command = std::process::Command::new("git");
    command
        .args(["init", "-q", "--bare", "-b", "main"])
        .arg(&remote_path);
    scrub_git_env(&mut command, world.project.config_home());
    command.status().expect("init bare remote");

    world
        .project
        .git(&["remote", "add", "origin", &remote_path.to_string_lossy()]);
    world.project.git(&["push", "-q", "origin", "main"]);

    Repo::at(remote_path)
}

fn remote_refs(remote: &Repo) -> Vec<String> {
    let mut command = std::process::Command::new("git");
    command
        .args(["--git-dir", &remote.path.to_string_lossy()])
        .args(["for-each-ref", "--format=%(refname)", "refs/tpl/"]);
    scrub_git_env(&mut command, remote.config_home());
    let listing = command.output().expect("list remote refs");
    String::from_utf8_lossy(&listing.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

/// A contributor who clones the project to fix a typo must not download
/// template state to do it, and must not have to know what a template ref is.
#[test]
fn a_plain_git_push_does_not_carry_template_refs() {
    let world = World::new();
    world.init(&[]).success();
    let remote = with_remote(&world);

    world.project.git(&["push", "origin", "main"]);

    assert!(
        remote_refs(&remote).is_empty(),
        "refs/tpl/* leaked into a plain push"
    );
}

#[test]
fn push_publishes_the_rendered_ref() {
    let world = World::new();
    world.init(&[]).success();
    let remote = with_remote(&world);

    tpl(&world.project, &["push"]).success();

    assert_eq!(remote_refs(&remote), [world.ref_name()]);
}

/// The refspec is passed per-invocation rather than written into `.git/config`,
/// so a plain `git fetch` stays plain for everyone who clones.
#[test]
fn pushing_does_not_rewrite_the_remotes_configuration() {
    let world = World::new();
    world.init(&[]).success();
    let _remote = with_remote(&world);
    let before = world
        .project
        .git(&["config", "--get-all", "remote.origin.fetch"]);

    tpl(&world.project, &["push"]).success();

    let after = world
        .project
        .git(&["config", "--get-all", "remote.origin.fetch"]);
    assert_eq!(before, after, "the user's refspecs were modified");
}

#[test]
fn fetch_retrieves_a_shared_ref_into_the_remote_namespace() {
    let world = World::new();
    world.init(&[]).success();
    let _remote = with_remote(&world);
    tpl(&world.project, &["push"]).success();

    // A second clone of the same project, standing in for a collaborator.
    let other = Repo::init_in(world.dir.path(), "other");
    other.git(&[
        "remote",
        "add",
        "origin",
        &world.dir.path().join("remote.git").to_string_lossy(),
    ]);
    other.git(&["fetch", "-q", "origin"]);
    other.git(&["checkout", "-q", "-b", "main", "origin/main"]);

    tpl(&other, &["fetch"]).success();

    assert!(
        other.has_ref("refs/remotes/origin/tpl/template"),
        "the shared ref should land under the remote's namespace"
    );
}

/// Fetching never moves the local ref. What to do about a newer remote copy is
/// a decision, and adopting someone else's rendering silently would be a
/// surprising thing for a fetch to do.
#[test]
fn fetch_does_not_move_the_local_ref() {
    let world = World::new();
    world.init(&[]).success();
    let _remote = with_remote(&world);
    tpl(&world.project, &["push"]).success();
    let before = world.project.rev_parse(&world.ref_name());

    tpl(&world.project, &["fetch"]).success();

    assert_eq!(world.project.rev_parse(&world.ref_name()), before);
}

#[test]
fn fetch_reports_that_the_ref_is_in_sync() {
    let world = World::new();
    world.init(&[]).success();
    let _remote = with_remote(&world);
    tpl(&world.project, &["push"]).success();

    tpl(&world.project, &["fetch"]).success().says("in sync");
}

#[test]
fn status_reports_how_the_local_ref_compares_to_the_remote() {
    let world = World::new();
    world.init(&[]).success();
    let _remote = with_remote(&world);
    tpl(&world.project, &["push"]).success();
    tpl(&world.project, &["fetch"]).success();

    tpl(&world.project, &["status"]).success().says("in sync");
}

#[test]
fn status_reports_being_ahead_of_the_remote() {
    let world = World::new();
    world.init(&[]).success();
    let _remote = with_remote(&world);
    tpl(&world.project, &["push"]).success();
    tpl(&world.project, &["fetch"]).success();

    world
        .template
        .repo
        .write("template/NEW.md.jinja", "# {{ project_name }}\n");
    world.template.repo.commit_all("feat: add a file");
    tpl(&world.project, &["update", "--defaults"]).success();

    tpl(&world.project, &["status"]).code(2).says("1 ahead");
}

/// The remote relation as data. `describe()` words it for a human — "1 ahead",
/// "in sync" — and a caller needs the two numbers instead, because "behind" is
/// the case that must block a push and prose is no way to detect it.
#[test]
fn status_json_reports_the_remote_relation() {
    let world = World::new();
    world.init(&[]).success();
    let _remote = with_remote(&world);
    tpl(&world.project, &["push"]).success();
    tpl(&world.project, &["fetch"]).success();

    let json = tpl(&world.project, &["--json", "status"]).success().json();
    assert_eq!(json["remote"]["ref"], "refs/remotes/origin/tpl/template");
    assert_eq!(json["remote"]["ahead"], 0);
    assert_eq!(json["remote"]["behind"], 0);

    // One local rendering the remote has not seen.
    world
        .template
        .repo
        .write("template/NEW.md.jinja", "# {{ project_name }}\n");
    world.template.repo.commit_all("feat: add a file");
    tpl(&world.project, &["update", "--defaults"]).success();

    let json = tpl(&world.project, &["--json", "status"]).code(2).json();
    assert_eq!(json["remote"]["ahead"], 1);
    assert_eq!(json["remote"]["behind"], 0);
}

/// A rendered ref is history others may have merged from; overwriting it
/// destroys the merge base their next update depends on.
#[test]
fn push_refuses_a_diverged_ref_and_says_how_to_reconcile() {
    let world = World::new();
    world.init(&[]).success();
    let _remote = with_remote(&world);
    tpl(&world.project, &["push"]).success();
    tpl(&world.project, &["fetch"]).success();

    // Someone else renders and publishes.
    let shared = world.project.rev_parse(&world.ref_name());
    let other = Repo::init_in(world.dir.path(), "other");
    other.git(&[
        "remote",
        "add",
        "origin",
        &world.dir.path().join("remote.git").to_string_lossy(),
    ]);
    other.git(&["fetch", "-q", "origin", "refs/tpl/*:refs/tpl/*"]);
    other.write("x.txt", "theirs\n");
    other.git(&["add", "-A"]);
    let their_tree = other.git(&["write-tree"]);
    let their_commit = other.git(&[
        "commit-tree",
        &their_tree,
        "-p",
        &shared,
        "-m",
        "tpl: their render",
    ]);
    other.git(&["update-ref", &world.ref_name(), &their_commit]);
    other.git(&[
        "push",
        "-q",
        "origin",
        &format!("{}:{}", world.ref_name(), world.ref_name()),
    ]);

    // Meanwhile we render locally.
    world
        .template
        .repo
        .write("template/NEW.md.jinja", "# {{ project_name }}\n");
    world.template.repo.commit_all("feat: add a file");
    tpl(&world.project, &["update", "--defaults"]).success();

    tpl(&world.project, &["fetch"]).success();
    tpl(&world.project, &["push"])
        .failure()
        .says("diverged")
        .says("git tpl fetch");
}

/// The default. Nothing has to be configured for it to work, and the
/// attachment is still fully described by the committed configuration.
#[test]
fn local_only_mode_needs_no_remote_at_all() {
    let world = World::new();

    world.init(&[]).success();

    assert!(world.project.has_ref(&world.ref_name()));
    tpl(&world.project, &["status"])
        .success()
        .silent_about("Remote:");
}

#[test]
fn push_without_a_remote_is_reported_clearly() {
    let world = World::new();
    world.init(&[]).success();

    tpl(&world.project, &["push"]).failure().says("origin");
}

#[test]
fn a_configured_remote_is_used() {
    let world = World::new();
    world.init(&[]).success();
    let remote = with_remote(&world);
    world
        .project
        .git(&["remote", "rename", "origin", "upstream"]);
    world.project.git(&["config", "tpl.remote", "upstream"]);

    tpl(&world.project, &["push"]).success().says("upstream");

    assert_eq!(remote_refs(&remote), [world.ref_name()]);
}

#[test]
fn a_command_line_remote_beats_the_configured_one() {
    let world = World::new();
    world.init(&[]).success();
    let remote = with_remote(&world);
    world
        .project
        .git(&["remote", "rename", "origin", "elsewhere"]);
    world
        .project
        .git(&["config", "tpl.remote", "does-not-exist"]);

    tpl(&world.project, &["push", "--remote", "elsewhere"]).success();

    assert_eq!(remote_refs(&remote), [world.ref_name()]);
}

#[test]
fn auto_push_publishes_after_an_update() {
    let world = World::new();
    world.init(&[]).success();
    let remote = with_remote(&world);
    world.project.git(&["config", "tpl.autoPush", "true"]);

    world
        .template
        .repo
        .write("template/NEW.md.jinja", "# {{ project_name }}\n");
    world.template.repo.commit_all("feat: add a file");

    tpl(&world.project, &["update", "--defaults"])
        .success()
        .says("Pushed");

    assert_eq!(remote_refs(&remote), [world.ref_name()]);
}

#[test]
fn a_dry_run_transfers_nothing() {
    let world = World::new();
    world.init(&[]).success();
    let remote = with_remote(&world);

    tpl(&world.project, &["push", "--dry-run"])
        .success()
        .says("Would push");

    assert!(remote_refs(&remote).is_empty());
}

// --- machine-readable output ------------------------------------------------

/// `state` and `relation` together: the words on stderr — "in sync", "1 ahead"
/// — are no way to detect the case that must block a push.
#[test]
fn fetch_reports_the_relation_as_json() {
    let world = World::new();
    world.init(&[]).success();
    let _remote = with_remote(&world);

    // Nothing published yet, so the remote has no copy of the ref at all —
    // which `null` says, and `{"ahead": 0, "behind": 0}` would not.
    let json = tpl(&world.project, &["--json", "fetch"]).success().json();
    assert_eq!(json["ok"], true);
    assert_eq!(json["state"], "absent");
    assert_eq!(json["relation"], serde_json::Value::Null);
    assert_eq!(json["remote"], "origin");

    tpl(&world.project, &["push"]).success();
    let json = tpl(&world.project, &["--json", "fetch"]).success().json();
    assert_eq!(json["state"], "synced");
    assert_eq!(json["relation"]["ahead"], 0);
    assert_eq!(json["relation"]["behind"], 0);
    assert_eq!(json["relation"]["synced"], true);
    assert_eq!(json["relation"]["diverged"], false);

    // One local rendering the remote has not seen.
    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();
    let json = tpl(&world.project, &["--json", "fetch"]).success().json();
    assert_eq!(json["state"], "ahead");
    assert_eq!(json["relation"]["ahead"], 1);
}

#[test]
fn push_reports_the_ref_as_json() {
    let world = World::new();
    world.init(&[]).success();
    let _remote = with_remote(&world);

    let json = tpl(&world.project, &["--json", "push"]).success().json();
    assert_eq!(json["ok"], true);
    assert_eq!(json["remote"], "origin");
    assert_eq!(json["ref"], world.ref_name());

    let json = tpl(&world.project, &["--json", "push", "--dry-run"])
        .success()
        .json();
    assert_eq!(json["dryRun"], true);
    assert_eq!(json["ref"], world.ref_name());
}

/// The remote copy is strictly ahead: someone else rendered and published, and
/// we have not. Fetching never moves the local ref, so this is the arm that
/// has to *report* rather than act — and `state` is how a caller knows.
#[test]
fn fetch_reports_being_behind_the_remote_as_json() {
    let world = World::new();
    world.init(&[]).success();
    let _remote = with_remote(&world);
    tpl(&world.project, &["push"]).success();

    // Someone else renders on top of the ref we share and publishes it.
    let shared = world.project.rev_parse(&world.ref_name());
    let other = Repo::init_in(world.dir.path(), "other");
    other.git(&[
        "remote",
        "add",
        "origin",
        &world.dir.path().join("remote.git").to_string_lossy(),
    ]);
    other.git(&["fetch", "-q", "origin", "refs/tpl/*:refs/tpl/*"]);
    other.write("x.txt", "theirs\n");
    other.git(&["add", "-A"]);
    let their_tree = other.git(&["write-tree"]);
    let their_commit = other.git(&[
        "commit-tree",
        &their_tree,
        "-p",
        &shared,
        "-m",
        "tpl: their render",
    ]);
    other.git(&["update-ref", &world.ref_name(), &their_commit]);
    other.git(&[
        "push",
        "-q",
        "origin",
        &format!("{}:{}", world.ref_name(), world.ref_name()),
    ]);

    let json = tpl(&world.project, &["--json", "fetch"]).success().json();

    assert_eq!(json["state"], "behind");
    assert_eq!(json["relation"]["behind"], 1);
    assert_eq!(json["relation"]["ahead"], 0);
    assert_eq!(json["relation"]["diverged"], false);

    // The local ref did not move. Adopting someone else's rendering is a
    // decision, and a fetch must not make it.
    assert_eq!(world.project.rev_parse(&world.ref_name()), shared);
}

/// A dry run still transfers nothing, and still says what it would have done.
#[test]
fn a_fetch_dry_run_reports_the_refspec_as_json() {
    let world = World::new();
    world.init(&[]).success();
    let _remote = with_remote(&world);

    let json = tpl(&world.project, &["--json", "fetch", "--dry-run"])
        .success()
        .json();

    assert_eq!(json["ok"], true);
    assert_eq!(json["dryRun"], true);
    assert_eq!(json["remote"], "origin");
    assert!(
        json["refspec"]
            .as_str()
            .expect("refspec")
            .contains("refs/tpl/"),
        "{json}"
    );
}
