//! `git tpl diff` and `git tpl merge`, and the conflict matrix.
//!
//! These exercise native Git integration and libgit2 initialization against real repositories. git-tpl
//! contributes no conflict resolution of its own, so what is being asserted is
//! that Git's behaviour reaches the user unmangled.

mod common;

use common::{World, tpl};

#[test]
fn detached_head_merges_support_both_ancestry_shapes() {
    use tpl::git::{GitBackend, LibGit2, Oid};
    for divergent in [false, true] {
        let repo = common::Repo::init("detached");
        repo.write("base", "base");
        repo.commit_all("base");
        let base = repo.rev_parse("HEAD");
        repo.write("incoming", "incoming");
        repo.commit_all("incoming");
        let target = Oid::parse(&repo.rev_parse("HEAD")).unwrap();
        repo.git(&["checkout", "--detach", &base]);
        if divergent {
            repo.write("local", "local");
            repo.commit_all("local");
        }
        let backend = LibGit2::discover(&repo.path).unwrap();
        backend.merge(target, "merge detached", true, &[]).unwrap();
        assert_eq!(repo.read("incoming"), "incoming");
        assert!(repo.git(&["rev-parse", "--abbrev-ref", "HEAD"]) == "HEAD");
        if !divergent {
            assert_eq!(repo.rev_parse("HEAD"), target.to_hex());
        }
    }
}

#[test]
fn fast_forward_preserves_local_files_and_publishes_only_after_safe_checkout() {
    use tpl::git::{GitBackend, LibGit2, Oid};
    for obstruction in ["tracked", "untracked", "ignored", "unrelated"] {
        let repo = common::Repo::init("checkout");
        repo.write("tracked", "base\n");
        repo.write("unrelated", "base\n");
        repo.commit_all("base");
        let base = repo.rev_parse("HEAD");
        repo.write("tracked", "incoming\n");
        repo.write("incoming", "incoming\n");
        repo.commit_all("incoming");
        let target = Oid::parse(&repo.rev_parse("HEAD")).unwrap();
        repo.git(&["reset", "--hard", &base]);
        if obstruction == "ignored" {
            repo.write(".git/info/exclude", "incoming\n");
        }
        let path = if ["ignored", "untracked"].contains(&obstruction) {
            "incoming"
        } else {
            obstruction
        };
        repo.write(path, "local\n");
        let backend = LibGit2::discover(&repo.path).unwrap();
        let result = backend.merge(target, "merge", true, &[]);
        assert_eq!(repo.read(path), "local\n", "{obstruction}");
        if obstruction == "unrelated" {
            result.unwrap();
            assert_eq!(repo.rev_parse("HEAD"), target.to_hex());
        } else {
            assert!(result.is_err(), "{obstruction}");
            assert_eq!(repo.rev_parse("HEAD"), base);
        }
    }
}

#[test]
fn unborn_checkout_refuses_untracked_and_ignored_obstructions() {
    use tpl::git::{GitBackend, LibGit2};
    for ignored in [false, true] {
        let repo = common::Repo::init("unborn");
        let backend = LibGit2::discover(&repo.path).unwrap();
        let blob = backend.write_blob(b"incoming\n").unwrap();
        let tree = backend
            .build_tree(&[tpl::git::TreeEntry {
                path: "incoming".into(),
                oid: blob,
                mode: tpl::git::FileMode::Blob,
            }])
            .unwrap();
        let target = backend.create_commit(tree, &[], "incoming").unwrap();
        repo.write("incoming", "local\n");
        if ignored {
            repo.write(".git/info/exclude", "incoming\n");
        }
        assert!(backend.merge(target, "merge", true, &[]).is_err());
        assert_eq!(backend.head_commit().unwrap(), None);
        assert_eq!(repo.read("incoming"), "local\n");
    }
}

#[test]
fn diff_reports_nothing_when_the_rendering_is_merged() {
    let world = World::new();
    world.init(&[]).success();

    // Every file the project has that the template does not produce shows as a
    // difference, which is documented — so assert on the template's own files.
    let output = tpl(&world.project, &["diff", "--name-only"]).success();
    assert!(
        !output.stdout.contains("Cargo.toml"),
        "a merged rendering must not differ in the files it owns:\n{}",
        output.stdout
    );
}

#[test]
fn diff_reports_what_a_merge_would_bring_in() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();

    let output = tpl(&world.project, &["diff", "--name-only"]).success();

    assert!(
        output.stdout.contains(".github/workflows/release.yml"),
        "{}",
        output.stdout
    );
    assert!(output.stdout.contains("README.md"), "{}", output.stdout);
}

#[test]
fn diff_emits_a_patch_on_stdout() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();

    let output = tpl(&world.project, &["diff"]).success();

    // A patch is data, so it goes to stdout — `git tpl diff | git apply` is an
    // obvious thing to want.
    assert!(
        output.stdout.contains("+Generated by git-tpl."),
        "{}",
        output.stdout
    );
    assert!(output.stdout.contains("@@"), "{}", output.stdout);
}

#[test]
fn diff_can_be_limited_to_paths() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();

    let output = tpl(&world.project, &["diff", "--", "README.md"]).success();

    assert!(output.stdout.contains("README.md"), "{}", output.stdout);
    assert!(!output.stdout.contains("release.yml"), "{}", output.stdout);
}

#[test]
fn diff_without_a_rendered_ref_says_what_to_do() {
    let world = World::new();
    world.init(&["--no-merge"]).success();
    world.project.git(&["update-ref", "-d", &world.ref_name()]);

    tpl(&world.project, &["diff"])
        .failure()
        .says("restore the original rendered ref");
}

// --- merging ----------------------------------------------------------------

#[test]
fn merge_brings_the_template_changes_into_the_worktree() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();

    tpl(&world.project, &["merge"]).success();

    assert!(world.project.exists(".github/workflows/release.yml"));
    assert!(
        world
            .project
            .read("README.md")
            .contains("Generated by git-tpl.")
    );
}

#[test]
fn merging_twice_is_a_no_op() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();
    tpl(&world.project, &["merge"]).success();

    tpl(&world.project, &["merge"])
        .success()
        .says("Already up to date");
}

/// The property that makes the whole model work: a file the template did not
/// touch is not part of the merge, however much the user changed it.
#[test]
fn a_user_edit_to_an_untouched_file_survives_without_a_conflict() {
    let world = World::new();
    world.init(&[]).success();

    world
        .project
        .write("Cargo.toml", "[package]\nname = \"renamed-by-hand\"\n");
    world.project.commit_all("chore: rename by hand");

    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();
    tpl(&world.project, &["merge"]).success();

    assert_eq!(
        world.project.read("Cargo.toml"),
        "[package]\nname = \"renamed-by-hand\"\n",
        "the template did not change Cargo.toml, so the user's version must stand"
    );
}

/// Both sides changed the same file in different places — Git combines them.
#[test]
fn non_conflicting_edits_to_one_file_are_combined() {
    let world = World::new();
    world.init(&[]).success();

    // The user prepends; the template appends.
    let readme = world.project.read("README.md");
    world.project.write(
        "README.md",
        &format!("> A banner added by the user.\n\n{readme}"),
    );
    world.project.commit_all("docs: add a banner");

    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();
    tpl(&world.project, &["merge"]).success();

    let merged = world.project.read("README.md");
    assert!(merged.contains("A banner added by the user."), "{merged}");
    assert!(merged.contains("Generated by git-tpl."), "{merged}");
    assert!(!merged.contains("<<<<<<<"), "{merged}");
}

/// Both sides changed the same region — a genuine ambiguity that wants a human.
#[test]
fn conflicting_edits_leave_ordinary_git_conflict_markers() {
    let world = World::new();
    world.init(&[]).success();

    let readme = world.project.read("README.md");
    world.project.write(
        "README.md",
        &format!("{readme}\nWritten by the user at the end.\n"),
    );
    world.project.commit_all("docs: append a line");

    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();

    tpl(&world.project, &["merge"])
        .success()
        .says("CONFLICT")
        .says("git merge --abort");

    let conflicted = world.project.read("README.md");
    assert!(conflicted.contains("<<<<<<<"), "{conflicted}");
    assert!(conflicted.contains("======="), "{conflicted}");
    assert!(conflicted.contains(">>>>>>>"), "{conflicted}");
}

/// The index is left exactly as Git leaves it, so every tool the user already
/// has applies — including the escape hatch.
#[test]
fn a_conflicted_merge_can_be_aborted_with_plain_git() {
    let world = World::new();
    world.init(&[]).success();

    let readme = world.project.read("README.md");
    world
        .project
        .write("README.md", &format!("{readme}\nWritten by the user.\n"));
    world.project.commit_all("docs: append a line");
    let before = world.project.working_state();

    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();
    tpl(&world.project, &["merge"]).success();

    world.project.git(&["merge", "--abort"]);

    assert_eq!(world.project.working_state(), before);
    assert_eq!(world.project.status(), "");
}

#[test]
fn a_conflicted_merge_can_be_resolved_and_committed() {
    let world = World::new();
    world.init(&[]).success();

    let readme = world.project.read("README.md");
    world
        .project
        .write("README.md", &format!("{readme}\nWritten by the user.\n"));
    world.project.commit_all("docs: append a line");

    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();
    tpl(&world.project, &["merge"]).success();

    world
        .project
        .write("README.md", "# demo\n\nResolved by hand.\n");
    world.project.commit_all("chore: resolve the merge");

    assert_eq!(world.project.status(), "");
    tpl(&world.project, &["status"]).success();
}

/// A file the user created is not something the template deleted, so a merge
/// must never remove it.
#[test]
fn a_user_created_file_is_never_removed_by_a_merge() {
    let world = World::new();
    world.init(&[]).success();
    world.project.write("MY-FILE.md", "mine\n");
    world.project.commit_all("docs: add my own file");

    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();
    tpl(&world.project, &["merge"]).success();

    assert!(world.project.exists("MY-FILE.md"));
    assert!(world.project.exists("NOTES.md"));
    assert!(world.project.exists(".config/git.tpl.toml"));
}

/// The user deliberately deleted a file the template still produces. The merge
/// base has it and neither side changed it, so it stays deleted.
#[test]
fn a_file_the_user_deleted_stays_deleted_when_the_template_did_not_touch_it() {
    let world = World::new();
    world.init(&[]).success();
    world.project.remove("ci.yml");
    world.project.commit_all("chore: drop the CI file");

    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();
    tpl(&world.project, &["merge"]).success();

    assert!(
        !world.project.exists("ci.yml"),
        "the template did not touch ci.yml, so the user's deletion must stand"
    );
}

#[test]
fn no_commit_stages_the_merge_without_committing() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();
    let before = world.project.rev_parse("HEAD");

    tpl(&world.project, &["merge", "--no-commit"]).success();

    assert_eq!(world.project.rev_parse("HEAD"), before);
    assert_ne!(world.project.status(), "", "changes should be staged");
}

#[test]
fn a_merge_message_can_be_overridden() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();

    tpl(
        &world.project,
        &["merge", "-m", "chore: take template changes"],
    )
    .success();

    assert!(
        world
            .project
            .commit_message("HEAD")
            .contains("chore: take template changes")
    );
}

/// The full cycle from the specification, run end to end.
#[test]
fn a_project_keeps_its_own_files_through_a_template_add_change_and_delete() {
    let world = World::new();

    world.init(&[]).success();
    assert!(world.project.exists("Cargo.toml"));

    // The user makes the project their own.
    world.project.write("src/main.rs", "fn main() {}\n");
    world.project.commit_all("feat: add a main");

    // The template moves on: adds, modifies, deletes.
    world.template.repo.write(
        "template/SECURITY.md.jinja",
        "# Security for {{ project_name }}\n",
    );
    world.template.repo.write(
        "template/src/lib.rs.jinja",
        "//! {{ project_name }}\n\npub fn version() -> &'static str { \"0.1.0\" }\n",
    );
    world.template.repo.remove("template/ci.yml");
    world.template.repo.commit_all("feat: revise the template");

    tpl(&world.project, &["status"])
        .code(2)
        .says("template has moved");

    let before = world.project.working_state();
    tpl(&world.project, &["update", "--defaults"]).success();
    assert_eq!(
        world.project.working_state(),
        before,
        "update touched the project"
    );

    tpl(&world.project, &["diff", "--stat"])
        .success()
        .says("insertions(+)");
    tpl(&world.project, &["merge"]).success();

    assert!(
        world.project.exists("SECURITY.md"),
        "the template's new file"
    );
    assert!(world.project.read("src/lib.rs").contains("pub fn version"));
    assert!(!world.project.exists("ci.yml"), "the template's deletion");
    assert!(world.project.exists("src/main.rs"), "the user's file");
    assert!(world.project.exists("NOTES.md"), "the user's other file");

    tpl(&world.project, &["status"]).success();
}

// --- machine-readable output ------------------------------------------------

#[test]
fn a_merge_reports_its_outcome_as_json() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();

    let json = tpl(&world.project, &["--json", "merge"]).success().json();

    assert_eq!(json["ok"], true);
    assert_eq!(json["ref"], world.ref_name());
    assert_eq!(json["id"], "template");
    // Whatever Git did, it is named by one field a caller can switch on.
    let result = json["result"].as_str().expect("result");
    assert!(
        ["merged", "fastForward", "staged"].contains(&result),
        "unexpected result {result}"
    );
}

/// The `update` hole (issue #53) in its `merge` form: doing nothing is still an
/// outcome, and a caller must be able to read it.
#[test]
fn an_up_to_date_merge_still_emits_an_envelope() {
    let world = World::new();
    world.init(&[]).success();

    let output = tpl(&world.project, &["--json", "merge"]).success();

    assert!(
        !output.stdout.trim().is_empty(),
        "stdout was empty\n--- stderr ---\n{}",
        output.stderr
    );
    assert_eq!(output.json()["result"], "upToDate");
}

/// A conflict is a success — the index is left for the user to resolve — so
/// the exit code cannot report it and `result` must.
#[test]
fn a_conflicted_merge_is_reported_as_json() {
    let world = World::new();
    world.init(&[]).success();

    world
        .project
        .write("README.md", "# mine\n\nentirely different\n");
    world.project.commit_all("chore: rewrite the readme");

    world.template.repo.write(
        "template/README.md.jinja",
        "# {{ project_name }}\n\nentirely other\n",
    );
    world.template.repo.commit_all("feat: rewrite the readme");
    tpl(&world.project, &["update", "--defaults"]).success();

    let json = tpl(&world.project, &["--json", "merge"]).success().json();

    assert_eq!(json["result"], "conflicted");
    let conflicts: Vec<&str> = json["conflicts"]
        .as_array()
        .expect("conflicts")
        .iter()
        .map(|p| p.as_str().expect("path"))
        .collect();
    assert!(conflicts.contains(&"README.md"), "{conflicts:?}");
}

/// Aborting is the undoing of a merge, so it gets its own `result`. Reporting
/// it as `upToDate` would tell a caller the opposite of what happened.
#[test]
fn an_aborted_merge_is_reported_as_json() {
    let world = World::new();
    world.init(&[]).success();

    world
        .project
        .write("README.md", "# mine\n\nentirely different\n");
    world.project.commit_all("chore: rewrite the readme");

    world.template.repo.write(
        "template/README.md.jinja",
        "# {{ project_name }}\n\nentirely other\n",
    );
    world.template.repo.commit_all("feat: rewrite the readme");
    tpl(&world.project, &["update", "--defaults"]).success();
    tpl(&world.project, &["merge"]).success();

    let head = world.project.rev_parse("HEAD");

    let json = tpl(&world.project, &["--json", "merge", "--abort"])
        .success()
        .json();

    assert_eq!(json["ok"], true);
    assert_eq!(json["result"], "aborted");
    assert_eq!(world.project.rev_parse("HEAD"), head, "HEAD moved");
    assert_eq!(world.project.status(), "", "the worktree is not clean");
}

#[test]
fn abort_without_a_merge_preserves_staged_and_unstaged_changes() {
    let world = World::new();
    world.init(&[]).success();
    world.project.write("NOTES.md", "staged\n");
    world.project.git(&["add", "NOTES.md"]);
    world.project.write("NOTES.md", "unstaged\n");
    let before = world.project.working_state();
    let json = tpl(&world.project, &["--json", "merge", "--abort"])
        .failure()
        .json();
    assert_eq!(json["error"]["code"], "tpl::git::no_merge");
    assert_eq!(world.project.working_state(), before);
}

#[test]
fn abort_preserves_unrelated_edits_and_native_failure_keeps_recovery_state() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();
    tpl(&world.project, &["merge", "--no-commit"]).success();
    world.project.write("NOTES.md", "my unrelated edit\n");
    // Editing a newly staged merge result makes Git refuse reset --merge.
    world
        .project
        .write(".github/workflows/release.yml", "my recovery edit\n");
    let before = world.project.working_state();
    let json = tpl(&world.project, &["--json", "merge", "--abort"])
        .failure()
        .json();
    assert_eq!(json["error"]["code"], "tpl::git::abort_failed");
    assert_eq!(world.project.working_state(), before);
    assert!(world.project.exists(".git/MERGE_HEAD"));
    world
        .project
        .git(&["restore", ".github/workflows/release.yml"]);
    tpl(&world.project, &["merge", "--abort"]).success();
    assert_eq!(world.project.read("NOTES.md"), "my unrelated edit\n");
}

// The script lives outside the rendered tree: only repository configuration,
// never a template manifest, grants permission to execute a merge driver.
fn driver_world(exit: u8) -> World {
    let world = World::with_template(
        "name = 'template'\n[questions.answer]\ntype = 'string'\ndefault = 'original'\n",
        &[
            ("file with spaces", "base\n"),
            (
                ".gitattributes",
                "\"file with spaces\" merge=fixture conflict-marker-size=11\n",
            ),
        ],
    );
    world.project.write(
        ".git/merge-driver.sh",
        &format!(
            "set -eu\n\
             cp \"$1\" \"$(git rev-parse --git-path driver-base)\"\n\
             cp \"$2\" \"$(git rev-parse --git-path driver-local)\"\n\
             cp \"$3\" \"$(git rev-parse --git-path driver-incoming)\"\n\
             printf '%s\\n%s\\n' \"$4\" \"$5\" > \"$(git rev-parse --git-path driver-args)\"\n\
             printf 'driver output on stdout\\n'\n\
             printf 'driver output on stderr\\n' >&2\n\
             printf 'resolved by driver\\n' > \"$2\"\n\
             exit {exit}\n"
        ),
    );
    // Forward slashes and shell quoting also work with Git for Windows' shell.
    let script = world
        .project
        .path
        .join(".git/merge-driver.sh")
        .to_string_lossy()
        .replace('\\', "/");
    world.project.git(&[
        "config",
        "merge.fixture.driver",
        &format!("sh '{}' %O %A %B %L %P", script.replace('\'', "'\\''")),
    ]);
    world.init(&[]).success();
    world.project.write("file with spaces", "local\n");
    world.project.commit_all("customize project");
    world
        .template
        .repo
        .write("template/file with spaces", "incoming\n");
    world.template.repo.commit_all("advance template");
    world
}

#[test]
fn configured_drivers_receive_git_inputs_and_control_the_merge_result() {
    for no_commit in [false, true] {
        let world = driver_world(0);
        tpl(&world.project, &["update", "--defaults"]).success();
        let before = world.project.rev_parse("HEAD");
        let incoming = world.project.rev_parse(&world.ref_name());
        // Branch defaults must not squash ancestry, refuse a divergent merge,
        // or autostash local edits despite the wrapper's explicit contract.
        world.project.git(&[
            "config",
            "branch.main.mergeOptions",
            "--squash --ff-only --autostash",
        ]);
        world.project.write("NOTES.md", "unrelated unstaged edit\n");
        let mut args = vec!["--json", "merge", "-m", "custom integration"];
        if no_commit {
            args.push("--no-commit");
        }
        let json = tpl(&world.project, &args).success().json();
        assert_eq!(json["result"], if no_commit { "staged" } else { "merged" });
        assert_eq!(world.project.read(".git/driver-base"), "base\n");
        assert_eq!(world.project.read(".git/driver-local"), "local\n");
        assert_eq!(world.project.read(".git/driver-incoming"), "incoming\n");
        assert_eq!(
            world.project.read(".git/driver-args"),
            "11\nfile with spaces\n"
        );
        assert_eq!(
            world.project.read("file with spaces"),
            "resolved by driver\n"
        );
        assert_eq!(
            world.project.git(&["show", ":file with spaces"]),
            "resolved by driver"
        );
        assert_eq!(world.project.read("NOTES.md"), "unrelated unstaged edit\n");
        assert!(!world.project.exists(".git/MERGE_AUTOSTASH"));
        if no_commit {
            assert_eq!(world.project.rev_parse("HEAD"), before);
            assert!(world.project.exists(".git/MERGE_HEAD"));
            world.project.git(&["commit", "-m", "finish integration"]);
        }
        assert_eq!(world.project.rev_parse("HEAD^1"), before);
        assert_eq!(world.project.rev_parse("HEAD^2"), incoming);
        assert_eq!(
            world.project.git(&["show", "HEAD:file with spaces"]),
            "resolved by driver"
        );
        if !no_commit {
            assert_eq!(
                world.project.commit_message("HEAD").trim(),
                "custom integration"
            );
        }
    }
}

#[test]
fn driver_conflicts_remain_resolvable_and_abort_preserves_rendered_history() {
    for abort in [false, true] {
        let world = driver_world(1);
        let before = world.project.rev_parse("HEAD");
        tpl(&world.project, &["update", "--defaults"]).success();
        let incoming = world.project.rev_parse(&world.ref_name());
        let json = tpl(&world.project, &["--json", "merge"]).success().json();
        assert_eq!(json["result"], "conflicted");
        assert_eq!(json["conflicts"], serde_json::json!(["file with spaces"]));
        assert_eq!(world.project.rev_parse("HEAD"), before);
        assert!(world.project.exists(".git/MERGE_HEAD"));
        assert!(!world.project.git(&["ls-files", "--unmerged"]).is_empty());
        if abort {
            tpl(&world.project, &["merge", "--abort"]).success();
            assert_eq!(world.project.read("file with spaces"), "local\n");
        } else {
            world.project.git(&["add", "file with spaces"]);
            world
                .project
                .git(&["commit", "-m", "resolve driver conflict"]);
            assert_eq!(world.project.rev_parse("HEAD^2"), incoming);
        }
        assert_eq!(world.project.rev_parse(&world.ref_name()), incoming);
        assert!(!world.project.exists(".git/MERGE_HEAD"));
    }
}

#[test]
fn fatal_driver_failures_are_errors_without_destructive_recovery() {
    let world = driver_world(129);
    tpl(&world.project, &["update", "--defaults"]).success();
    world.project.write("NOTES.md", "unrelated unstaged edit\n");
    let before = world.project.rev_parse("HEAD");
    let json = tpl(&world.project, &["--json", "merge"]).failure().json();
    assert_eq!(json["error"]["code"], "tpl::git::merge_failed");
    assert_eq!(world.project.rev_parse("HEAD"), before);
    assert_eq!(world.project.read("NOTES.md"), "unrelated unstaged edit\n");
    // Git can fail before writing MERGE_HEAD; either state must be usable.
    if world.project.exists(".git/MERGE_HEAD") {
        tpl(&world.project, &["merge", "--abort"]).success();
    }
    world
        .project
        .git(&["config", "merge.fixture.driver", "true"]);
    tpl(&world.project, &["merge"]).success();
}

#[test]
fn all_explicit_integration_commands_honor_drivers_and_leave_answers_unstaged() {
    for command in ["pull", "update", "switch"] {
        let world = driver_world(0);
        let source = world.template.source();
        let mut args = vec![command];
        if command == "switch" {
            args.push(&source);
        }
        if command != "pull" {
            args.push("--merge");
        }
        args.extend(["--defaults", "--answer", "answer=changed"]);
        tpl(&world.project, &args).success();
        assert_eq!(
            world.project.read("file with spaces"),
            "resolved by driver\n"
        );
        assert!(
            world
                .project
                .read(".config/git.tpl.toml")
                .contains("changed")
        );
        assert!(
            world
                .project
                .git(&["diff", "--cached", "--name-only"])
                .is_empty()
        );
        assert_eq!(
            world.project.git(&["diff", "--name-only"]),
            ".config/git.tpl.toml"
        );
    }
}

#[test]
fn rendering_and_preview_do_not_execute_merge_drivers() {
    let world = driver_world(0);
    assert!(
        !world.project.exists(".git/driver-base"),
        "init executed the driver"
    );
    tpl(&world.project, &["update", "--defaults"]).success();
    tpl(&world.project, &["--json", "diff"]).success();
    assert!(!world.project.exists(".git/driver-base"));
}

#[test]
fn explicit_merge_refuses_staged_changes_and_active_operations() {
    let world = driver_world(0);
    tpl(&world.project, &["update", "--defaults"]).success();
    world.project.write("NOTES.md", "staged user edit\n");
    world.project.git(&["add", "NOTES.md"]);
    let before = world.project.working_state();
    assert_eq!(
        tpl(&world.project, &["--json", "merge"])
            .failure()
            .error_code(),
        "tpl::git::dirty_index"
    );
    assert_eq!(world.project.working_state(), before);
    world.project.git(&["restore", "--staged", "NOTES.md"]);
    tpl(&world.project, &["merge", "--no-commit"]).success();
    let before = world.project.working_state();
    assert_eq!(
        tpl(&world.project, &["--json", "merge"])
            .failure()
            .error_code(),
        "tpl::git::repository_busy"
    );
    assert_eq!(world.project.working_state(), before);
}

#[test]
fn native_merge_protects_local_files_before_publishing_fast_forwards() {
    use tpl::git::{GitBackend, LibGit2, Oid};
    for obstruction in ["tracked", "untracked", "ignored", "unrelated"] {
        let repo = common::Repo::init("native checkout");
        repo.write("tracked", "base\n");
        repo.write("unrelated", "base\n");
        repo.commit_all("base");
        let base = repo.rev_parse("HEAD");
        repo.write("tracked", "incoming\n");
        repo.write("incoming", "incoming\n");
        repo.commit_all("incoming");
        let target = Oid::parse(&repo.rev_parse("HEAD")).unwrap();
        repo.git(&["reset", "--hard", &base]);
        if obstruction == "ignored" {
            repo.write(".git/info/exclude", "incoming\n");
        }
        let path = if ["ignored", "untracked"].contains(&obstruction) {
            "incoming"
        } else {
            obstruction
        };
        repo.write(path, "local\n");
        let result = LibGit2::discover(&repo.path)
            .unwrap()
            .merge_explicit(target, "merge", true);
        assert_eq!(repo.read(path), "local\n", "{obstruction}");
        if obstruction == "unrelated" {
            result.unwrap();
            assert_eq!(repo.rev_parse("HEAD"), target.to_hex());
        } else {
            assert!(result.is_err(), "{obstruction}");
            assert_eq!(repo.rev_parse("HEAD"), base);
        }
    }
}

#[test]
fn native_merges_target_linked_worktrees_from_subdirectories() {
    let world = driver_world(0);
    tpl(&world.project, &["update", "--defaults"]).success();
    let linked = world.dir.path().join("linked worktree");
    world
        .project
        .git(&["worktree", "add", "-b", "linked", linked.to_str().unwrap()]);
    let subdir = linked.join("subdirectory");
    std::fs::create_dir(&subdir).unwrap();
    common::tpl_outside(&subdir, world.project.config_home(), &["--json", "merge"]).success();
    assert_eq!(
        std::fs::read_to_string(linked.join("file with spaces")).unwrap(),
        "resolved by driver\n"
    );
    assert_eq!(world.project.read("file with spaces"), "local\n");
}

#[test]
fn rejected_merge_commits_report_failure_and_retain_abort_state() {
    let world = driver_world(0);
    tpl(&world.project, &["update", "--defaults"]).success();
    world.project.write(
        ".git/hooks/pre-merge-commit",
        "#!/bin/sh\nprintf 'reject integration\\n' >&2\nexit 1\n",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            world.project.path.join(".git/hooks/pre-merge-commit"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let before = world.project.rev_parse("HEAD");
    let output = tpl(&world.project, &["--json", "merge"])
        .failure()
        .says("reject integration");
    assert_eq!(output.error_code(), "tpl::git::merge_failed");
    assert_eq!(world.project.rev_parse("HEAD"), before);
    assert!(world.project.exists(".git/MERGE_HEAD"));
    tpl(&world.project, &["merge", "--abort"]).success();
    assert_eq!(world.project.read("file with spaces"), "local\n");
}

#[test]
fn unavailable_native_git_reports_failure_without_touching_the_repository() {
    let world = driver_world(0);
    tpl(&world.project, &["update", "--defaults"]).success();
    let before = world.project.working_state();
    let empty_path = tempfile::tempdir().unwrap();
    let output = common::tpl_with_env(
        &world.project,
        &["--json", "merge"],
        &[("PATH", empty_path.path().to_str().unwrap())],
    )
    .failure();
    assert_eq!(output.error_code(), "tpl::git::merge_failed");
    assert_eq!(world.project.working_state(), before);
}

#[test]
fn native_detached_merges_preserve_both_ancestry_shapes_and_no_commit_fast_forwards() {
    use tpl::git::{GitBackend, LibGit2, MergeOutcome, Oid};
    for divergent in [false, true] {
        let repo = common::Repo::init("detached native");
        repo.write("base", "base");
        repo.commit_all("base");
        let base = repo.rev_parse("HEAD");
        repo.write("incoming", "incoming");
        repo.commit_all("incoming");
        let target = Oid::parse(&repo.rev_parse("HEAD")).unwrap();
        repo.git(&["checkout", "--detach", &base]);
        if divergent {
            repo.write("local", "local");
            repo.commit_all("local");
        }
        let before = repo.rev_parse("HEAD");
        let outcome = LibGit2::discover(&repo.path)
            .unwrap()
            .merge_explicit(target, "merge detached", divergent)
            .unwrap();
        assert_eq!(repo.read("incoming"), "incoming");
        assert_eq!(repo.git(&["rev-parse", "--abbrev-ref", "HEAD"]), "HEAD");
        if divergent {
            assert!(matches!(outcome, MergeOutcome::Merged { .. }));
            assert_eq!(repo.rev_parse("HEAD^1"), before);
            assert_eq!(repo.rev_parse("HEAD^2"), target.to_hex());
        } else {
            assert!(matches!(outcome, MergeOutcome::FastForward { .. }));
            assert_eq!(repo.rev_parse("HEAD"), target.to_hex());
        }
    }
}

#[test]
fn initialization_does_not_invoke_a_driver_even_for_conflicting_input() {
    let world = World::with_template("name = 'template'", &[("out", "incoming\n")]);
    world.project.write("out", "local\n");
    world.project.write(".gitattributes", "out merge=fixture\n");
    world.project.commit_all("conflicting local input");
    world.project.git(&[
        "config",
        "merge.fixture.driver",
        "touch .git/driver-invoked",
    ]);
    world.init(&[]).success();
    assert!(world.project.exists(".git/MERGE_HEAD"));
    assert!(!world.project.exists(".git/driver-invoked"));
}
