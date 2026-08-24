//! `git tpl update` — re-rendering and advancing the ref.
//!
//! The invariant these tests exist to protect: **update changes one ref and
//! nothing else.**

mod common;

use common::{World, tpl, tpl_colored};

/// The single most important test in the suite.
#[test]
fn update_does_not_touch_head_the_index_or_the_worktree() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();

    let before = world.project.working_state();

    tpl(&world.project, &["update", "--defaults"]).success();

    let after = world.project.working_state();
    assert_eq!(before.head, after.head, "HEAD moved");
    assert_eq!(before.index, after.index, "the index changed");
    assert_eq!(before.worktree, after.worktree, "the worktree changed");
    assert_eq!(world.project.status(), "", "the worktree is not clean");
}

#[test]
fn update_advances_the_rendered_ref() {
    let world = World::new();
    world.init(&[]).success();
    let first = world.project.rev_parse(&world.ref_name());

    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();

    let second = world.project.rev_parse(&world.ref_name());
    assert_ne!(first, second);
}

/// Append-only. Rewriting would destroy the merge base the branch already
/// shares with the ref, and the next merge would conflict on everything.
#[test]
fn a_new_rendering_has_the_previous_one_as_its_parent() {
    let world = World::new();
    world.init(&[]).success();
    let first = world.project.rev_parse(&world.ref_name());

    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();

    let parent = world.project.rev_parse(&format!("{}^", world.ref_name()));
    assert_eq!(parent, first);
}

/// The determinism guarantee, observed from outside: identical inputs produce
/// no commit, so the ref grows only when something real changed.
#[test]
fn an_unchanged_template_produces_no_commit() {
    let world = World::new();
    world.init(&[]).success();
    let before = world.project.rev_parse(&world.ref_name());

    tpl(&world.project, &["update", "--defaults"])
        .success()
        .says("Already up to date");

    assert_eq!(world.project.rev_parse(&world.ref_name()), before);
}

#[test]
fn updating_twice_in_a_row_still_produces_no_commit() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();

    tpl(&world.project, &["update", "--defaults"]).success();
    let after_first = world.project.rev_parse(&world.ref_name());

    tpl(&world.project, &["update", "--defaults"]).success();

    assert_eq!(world.project.rev_parse(&world.ref_name()), after_first);
}

/// Editing an answer and updating is the supported way to change your mind
/// about a choice made at init time.
#[test]
fn changing_an_answer_produces_a_new_rendering() {
    let world = World::new();
    world.init(&[]).success();
    let before = world.project.rev_parse(&world.ref_name());

    let config = world.project.read(".config/git.tpl.toml");
    world.project.write(
        ".config/git.tpl.toml",
        &config.replace("license = \"MIT\"", "license = \"Apache-2.0\""),
    );

    tpl(&world.project, &["update", "--defaults"]).success();

    let after = world.project.rev_parse(&world.ref_name());
    assert_ne!(after, before);
    assert_eq!(
        world.project.rev_parse(&format!("{}^", world.ref_name())),
        before,
        "an answer change is still append-only"
    );

    let rendered = world
        .project
        .git(&["show", &format!("{}:Cargo.toml", world.ref_name())]);
    assert!(rendered.contains("Apache-2.0"), "{rendered}");
}

#[test]
fn the_new_rendering_records_the_new_template_revision() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();

    tpl(&world.project, &["update", "--defaults"]).success();

    let message = world.project.commit_message(&world.ref_name());
    assert!(
        message.contains(&world.template.repo.rev_parse("HEAD")),
        "{message}"
    );
}

#[test]
fn update_reports_what_changed() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();

    tpl(&world.project, &["update", "--defaults"])
        .success()
        .says("added     .github/workflows/release.yml")
        .says("modified  README.md")
        .says("Your working tree was not modified.");
}

#[test]
fn a_dry_run_writes_nothing() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();

    let before_ref = world.project.rev_parse(&world.ref_name());
    let before_state = world.project.working_state();

    tpl(&world.project, &["update", "--defaults", "--dry-run"])
        .success()
        .says("Nothing was written");

    assert_eq!(world.project.rev_parse(&world.ref_name()), before_ref);
    assert_eq!(world.project.working_state(), before_state);
}

/// A dry run and a real run describe the same revision the same way, or the
/// "Revision" line means two different things depending on a flag. The dry-run
/// path used to print a bare branch name, with no commit — which is precisely
/// the thing that line exists to show.
#[test]
fn a_dry_run_describes_the_revision_the_way_a_real_run_does() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();

    let revision = world.template.repo.rev_parse("HEAD");
    let short = &revision[..7];

    tpl(&world.project, &["update", "--defaults", "--dry-run"])
        .success()
        .says(short);

    tpl(&world.project, &["update", "--defaults"])
        .success()
        .says(short);
}

// --- what the template can do ----------------------------------------------

#[test]
fn a_file_deleted_from_the_template_is_deleted_from_the_rendering() {
    let world = World::new();
    world.init(&[]).success();

    world.template.repo.remove("template/ci.yml");
    world.template.repo.commit_all("chore: drop the CI file");

    tpl(&world.project, &["update", "--defaults"])
        .success()
        .says("deleted   ci.yml");

    assert!(
        !world
            .project
            .tree_paths(&world.ref_name())
            .contains(&"ci.yml".to_string())
    );
}

#[test]
fn a_file_added_to_the_template_appears_in_the_rendering() {
    let world = World::new();
    world.init(&[]).success();

    world.template.repo.write(
        "template/CONTRIBUTING.md.jinja",
        "# Contributing to {{ project_name }}\n",
    );
    world.template.repo.commit_all("docs: add contributing");

    tpl(&world.project, &["update", "--defaults"]).success();

    let rendered = world
        .project
        .git(&["show", &format!("{}:CONTRIBUTING.md", world.ref_name())]);
    assert_eq!(rendered, "# Contributing to demo");
}

/// A question added since the last render has no recorded answer, so it is
/// asked — or, with `--defaults`, takes its default — and written back.
#[test]
fn a_question_added_by_the_template_is_answered_and_recorded() {
    let world = World::new();
    world.init(&[]).success();

    let manifest = world.template.repo.read("template.toml");
    world.template.repo.write(
        "template.toml",
        &format!("{manifest}\n[questions.edition]\ntype = \"string\"\ndefault = \"2024\"\n"),
    );
    world.template.repo.write(
        "template/Cargo.toml.jinja",
        "[package]\nname = \"{{ package_name }}\"\nedition = \"{{ edition }}\"\n",
    );
    world.template.repo.commit_all("feat: ask for the edition");

    tpl(&world.project, &["update", "--defaults"]).success();

    assert!(
        world
            .project
            .read(".config/git.tpl.toml")
            .contains("edition = \"2024\"")
    );
}

// --- revision selection -----------------------------------------------------

#[test]
fn a_tag_can_be_rendered_instead_of_a_branch() {
    let world = World::new();
    world.template.repo.git(&["tag", "v1.0.0"]);
    world.move_template();

    world.init(&["--ref", "v1.0.0"]).success();

    assert!(
        !world.project.exists(".github/workflows/release.yml"),
        "v1.0.0 predates the release workflow"
    );
    assert!(
        world
            .project
            .commit_message(&world.ref_name())
            .contains("Template-Ref: v1.0.0")
    );
}

#[test]
fn a_commit_sha_can_be_rendered() {
    let world = World::new();
    let first = world.template.repo.rev_parse("HEAD");
    world.move_template();

    world.init(&["--ref", &first]).success();

    assert!(!world.project.exists(".github/workflows/release.yml"));
}

/// `--ref` answers "what would v2 look like?" — a question, not a decision, so
/// it must not rewrite the configuration.
#[test]
fn ref_on_the_command_line_does_not_change_the_configuration() {
    let world = World::new();
    world.init(&[]).success();
    let first = world.template.repo.rev_parse("HEAD");
    world.move_template();

    tpl(&world.project, &["update", "--defaults", "--ref", &first]).success();

    let config = world.project.read(".config/git.tpl.toml");
    assert!(!config.contains(&first), "{config}");
}

/// A developer should never have to publish a template release to test a
/// change against a project.
#[test]
fn dirty_renders_the_templates_uncommitted_working_tree() {
    let world = World::new();
    world.init(&[]).success();

    world.template.repo.write(
        "template/README.md.jinja",
        "# {{ project_name }}\n\nUncommitted.\n",
    );

    tpl(&world.project, &["update", "--defaults"])
        .success()
        .says("Already up to date");

    tpl(&world.project, &["update", "--defaults", "--dirty"]).success();

    let rendered = world
        .project
        .git(&["show", &format!("{}:README.md", world.ref_name())]);
    assert!(rendered.contains("Uncommitted."), "{rendered}");
}

/// Nobody can reproduce a tree rendered from an uncommitted directory, and the
/// commit must say so rather than claiming a revision it does not have.
#[test]
fn a_dirty_rendering_is_marked_as_such() {
    let world = World::new();
    world.init(&[]).success();
    world.template.repo.write(
        "template/README.md.jinja",
        "# {{ project_name }}\n\nUncommitted.\n",
    );

    tpl(&world.project, &["update", "--defaults", "--dirty"]).success();

    let message = world.project.commit_message(&world.ref_name());
    assert!(message.contains("Template-Dirty: true"), "{message}");
    assert!(message.contains("Template-Ref: <worktree>"), "{message}");
}

#[test]
fn update_without_a_configuration_says_what_to_do() {
    let world = World::new();

    tpl(&world.project, &["update", "--defaults"])
        .failure()
        .says("git tpl init");
}

// --- machine-readable output ------------------------------------------------

/// A ref that is not there is not an error — a clone without `refs/tpl/*` is
/// the ordinary case — but the resulting commit shares no ancestry with what
/// the branch merged, so a later `git tpl merge` can conflict on every file.
/// Said, rather than discovered during that merge.
#[test]
fn an_update_with_no_local_ref_says_it_started_a_new_history() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    world.project.git(&["update-ref", "-d", &world.ref_name()]);

    let output = tpl(&world.project, &["--json", "update", "--defaults"]).success();

    assert_eq!(output.json()["startedNewHistory"], true);
}

/// The same fact, for a person. `--json` silences prose, so it takes a second
/// run to see it.
#[test]
fn an_update_with_no_local_ref_warns_a_human_too() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    world.project.git(&["update-ref", "-d", &world.ref_name()]);

    tpl(&world.project, &["update", "--defaults"])
        .success()
        .says("started a new history");
}

#[test]
fn an_update_onto_an_existing_ref_does_not_claim_a_new_history() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();

    let json = tpl(&world.project, &["--json", "update", "--defaults"])
        .success()
        .json();

    assert_eq!(json["startedNewHistory"], false);
}

/// The regression for issue #53.
///
/// `--json update` on the *most common* outcome used to print nothing at all,
/// so `git tpl --json update | jq .ok` failed to parse and a caller could not
/// tell "up to date" apart from "the binary produced no output".
#[test]
fn an_unchanged_template_still_reports_up_to_date_as_json() {
    let world = World::new();
    world.init(&[]).success();

    let output = tpl(&world.project, &["--json", "update", "--defaults"]).success();

    assert!(
        !output.stdout.trim().is_empty(),
        "stdout was empty; --json must always emit an envelope\n--- stderr ---\n{}",
        output.stderr
    );
    let json = output.json();
    assert_eq!(json["ok"], true);
    assert_eq!(json["result"], "upToDate");
    assert_eq!(json["ref"], world.ref_name());
}

/// `result` is the field a caller branches on: both outcomes exit zero, so the
/// exit code cannot tell them apart.
#[test]
fn an_update_reports_its_changes_as_json() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();

    let json = tpl(&world.project, &["--json", "update", "--defaults"])
        .success()
        .json();

    assert_eq!(json["ok"], true);
    assert_eq!(json["result"], "updated");
    assert_eq!(json["ref"], world.ref_name());
    assert_eq!(json["commit"], world.project.rev_parse(&world.ref_name()));
    // Nothing was pushed, and `null` says so without a caller having to guess
    // from an absent key.
    assert_eq!(json["pushed"], serde_json::Value::Null);

    let paths: Vec<&str> = json["changes"]
        .as_array()
        .expect("changes")
        .iter()
        .map(|c| c["path"].as_str().expect("path"))
        .collect();
    assert!(
        paths.contains(&".github/workflows/release.yml"),
        "{paths:?}"
    );

    // The unpadded kind, never the column-aligned `"added   "` label.
    let kinds: Vec<&str> = json["changes"]
        .as_array()
        .expect("changes")
        .iter()
        .map(|c| c["kind"].as_str().expect("kind"))
        .collect();
    assert!(kinds.contains(&"added"), "{kinds:?}");
    assert!(kinds.contains(&"modified"), "{kinds:?}");
}

/// A dry run is still a run. Leaving it silent under `--json` would reopen the
/// hole on the path a caller uses precisely because it is safe.
#[test]
fn a_dry_run_reports_what_would_change_as_json() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    let before = world.project.rev_parse(&world.ref_name());

    let json = tpl(
        &world.project,
        &["--json", "update", "--defaults", "--dry-run"],
    )
    .success()
    .json();

    assert_eq!(json["ok"], true);
    assert_eq!(json["dryRun"], true);
    assert_eq!(json["result"], "wouldUpdate");
    assert!(!json["changes"].as_array().expect("changes").is_empty());
    assert_eq!(
        world.project.rev_parse(&world.ref_name()),
        before,
        "a dry run advanced the ref"
    );
}

/// The `upToDate` arm of a dry run, which is the same silent-success shape
/// issue #53 was about: nothing to do is still an outcome.
#[test]
fn a_dry_run_with_nothing_to_do_reports_up_to_date_as_json() {
    let world = World::new();
    world.init(&[]).success();

    let output = tpl(
        &world.project,
        &["--json", "update", "--defaults", "--dry-run"],
    )
    .success();

    assert!(
        !output.stdout.trim().is_empty(),
        "stdout was empty\n--- stderr ---\n{}",
        output.stderr
    );
    let json = output.json();
    assert_eq!(json["dryRun"], true);
    assert_eq!(json["result"], "upToDate");
    assert_eq!(json["changes"], serde_json::json!([]));
}

// --- migrations --------------------------------------------------------
//
// See `docs/adr/024-template-migrations.md`. `World::add_migration` writes
// directly to the template repository's `migrations/` directory; these
// tests additionally rename or edit files under `template/` themselves, so
// that the *rendered* output actually changes shape the way a real
// migration's companion template edit would.

/// A move with nothing else changing needs no commit of its own: the final
/// rendered commit already *is* the pure rename, so `update` writes exactly
/// one commit — same as any other update.
#[test]
fn a_pure_move_produces_a_single_commit() {
    let world = World::new();
    world.init(&[]).success();
    let before = world.project.rev_parse(&world.ref_name());

    // The template author actually moves the file, so the fresh render
    // naturally stops producing `README.md` and starts producing
    // `docs/README.md` — with the same content, since nothing else changed.
    let readme = world.template.repo.read("template/README.md.jinja");
    world.template.repo.remove("template/README.md.jinja");
    world
        .template
        .repo
        .write("template/docs/README.md.jinja", &readme);
    world.add_migration(
        "000-move-readme.toml",
        "[[moves]]\nfrom = \"README.md\"\nto = \"docs/README.md\"\n",
    );

    tpl(&world.project, &["update", "--defaults"]).success();

    let after = world.project.rev_parse(&world.ref_name());
    assert_ne!(after, before);
    assert_eq!(
        world.project.rev_parse(&format!("{}^", world.ref_name())),
        before,
        "a pure move must not insert an intermediate commit"
    );

    let paths = world.project.tree_paths(&world.ref_name());
    assert!(!paths.contains(&"README.md".to_string()), "{paths:?}");
    assert!(paths.contains(&"docs/README.md".to_string()), "{paths:?}");
}

/// A move that lands alongside an unrelated content change needs the
/// rename split into its own commit first — otherwise a plain `git merge`'s
/// similarity heuristic has two things to explain at once and may not see a
/// rename at all.
#[test]
fn a_move_alongside_another_change_gets_an_intermediate_rename_commit() {
    let world = World::new();
    world.init(&[]).success();
    let before = world.project.rev_parse(&world.ref_name());

    let readme = world.template.repo.read("template/README.md.jinja");
    world.template.repo.remove("template/README.md.jinja");
    world
        .template
        .repo
        .write("template/docs/README.md.jinja", &readme);
    // Unrelated to the move: a new file the migration says nothing about.
    world
        .template
        .repo
        .write("template/.github/workflows/release.yml", "name: Release\n");
    world.add_migration(
        "000-move-readme.toml",
        "[[moves]]\nfrom = \"README.md\"\nto = \"docs/README.md\"\n",
    );

    let json = tpl(&world.project, &["--json", "update", "--defaults"])
        .success()
        .json();

    let tip = world.project.rev_parse(&world.ref_name());
    let rename_commit = world.project.rev_parse(&format!("{}^", world.ref_name()));
    let original_tip = world.project.rev_parse(&format!("{}^^", world.ref_name()));
    assert_eq!(original_tip, before);
    assert_eq!(
        json["movedCommit"].as_str().expect("movedCommit"),
        rename_commit
    );

    // The rename commit moves `README.md` and nothing else: the new workflow
    // file is not there yet, and neither is any other content change.
    let renamed_paths = world.project.tree_paths(&rename_commit);
    assert!(renamed_paths.contains(&"docs/README.md".to_string()));
    assert!(!renamed_paths.contains(&"README.md".to_string()));
    assert!(!renamed_paths.contains(&".github/workflows/release.yml".to_string()));

    // The final commit carries everything, exactly as an ordinary update
    // would if the move were not there at all.
    let final_paths = world.project.tree_paths(&tip);
    assert!(final_paths.contains(&"docs/README.md".to_string()));
    assert!(final_paths.contains(&".github/workflows/release.yml".to_string()));
}

/// A migration's message forces a commit even when the rendered output does
/// not change at all — otherwise the provenance trailer never advances past
/// it, and the same migration would resurface on every later update.
#[test]
fn a_message_only_migration_forces_a_commit() {
    let world = World::new();
    world.init(&[]).success();
    let before = world.project.rev_parse(&world.ref_name());

    world.add_migration(
        "000-note.toml",
        "message = \"0.4 split config.rs into a module.\"\n",
    );

    let json = tpl(&world.project, &["--json", "update", "--defaults"])
        .success()
        .json();

    assert_eq!(json["result"], "updated");
    assert_ne!(world.project.rev_parse(&world.ref_name()), before);
    assert_eq!(
        json["migrations"][0]["message"],
        "0.4 split config.rs into a module."
    );
    assert_eq!(json["migrations"][0]["moves"], serde_json::json!([]));
}

/// The message is shown to a person, sanitised and framed exactly like a
/// template's `init`-time note.
#[test]
fn a_migration_message_is_shown_in_an_attributed_block() {
    let world = World::new();
    world.init(&[]).success();

    world.add_migration(
        "000-note.toml",
        "message = \"0.4 split config.rs into a module.\"\n",
    );

    tpl(&world.project, &["update", "--defaults"])
        .success()
        .says("from the template")
        .says("0.4 split config.rs into a module.");
}

/// The property the whole discovery mechanism depends on: a migration is
/// crossed exactly once. The next `update`'s "old" tree already contains it,
/// so the diff that discovers new migrations is empty.
#[test]
fn a_migration_does_not_resurface_on_a_later_update() {
    let world = World::new();
    world.init(&[]).success();

    world.add_migration(
        "000-note.toml",
        "message = \"0.4 split config.rs into a module.\"\n",
    );
    tpl(&world.project, &["update", "--defaults"]).success();

    // Nothing else changed, and the migration was already crossed: this is
    // an ordinary up-to-date run.
    tpl(&world.project, &["update", "--defaults"])
        .success()
        .says("Already up to date")
        .silent_about("0.4 split config.rs into a module.");
}

/// Colour survives on a terminal, exactly as a template's `init`-time note
/// does — a migration message with no emphasis is one people stop reading.
#[test]
fn a_migration_message_keeps_its_colour_on_a_terminal() {
    let world = World::new();
    world.init(&[]).success();

    world.add_migration(
        "000-note.toml",
        "message = \"\\u001b[1mbold\\u001b[0m and \\u001b[2Jgone\"\n",
    );

    let output = tpl_colored(&world.project, &["update", "--defaults"]).success();
    assert!(
        output.stderr.contains("\x1b[1m"),
        "styling must survive on a terminal:\n{:?}",
        output.stderr
    );
    // ...but the screen-clear still does not.
    assert!(!output.stderr.contains("\x1b[2J"));
    assert!(output.stderr.contains("bold"));
}

/// `message_file` is repository-root-relative, read from the whole template
/// tree, exactly like `note_file`.
#[test]
fn a_migration_message_file_is_shown() {
    let world = World::new();
    world.init(&[]).success();

    world
        .template
        .repo
        .write("NEXT-STEPS.md", "Braces {{ stay }}.\n");
    world.add_migration("000-note.toml", "message_file = \"NEXT-STEPS.md\"\n");

    tpl(&world.project, &["update", "--defaults"])
        .success()
        .says("Braces {{ stay }}.");
}

/// Rendered if and only if the path ends in `.jinja` — the same rule the
/// renderer applies to files, and `note_file` applies to a note.
#[test]
fn a_migration_message_file_is_rendered_when_it_ends_in_jinja() {
    let world = World::new();
    world.init(&[]).success();

    world.template.repo.write(
        "NEXT-STEPS.md.jinja",
        "Set up {{ project_name }} by running bootstrap.\n",
    );
    world.add_migration("000-note.toml", "message_file = \"NEXT-STEPS.md.jinja\"\n");

    tpl(&world.project, &["update", "--defaults"])
        .success()
        .says("Set up demo by running bootstrap.");
}

/// A path that renders to nothing is the migration choosing to say nothing
/// for these answers — the same reading `template_note` gives `note_file`.
#[test]
fn a_migration_message_file_path_that_renders_empty_shows_no_message() {
    let world = World::new();
    world.init(&[]).success();

    world
        .template
        .repo
        .write("CI.md", "CI notes, never shown.\n");
    world.add_migration(
        "000-note.toml",
        "message_file = \"{% if false %}CI.md{% endif %}\"\n",
    );

    let output = tpl(&world.project, &["update", "--defaults"]).success();
    assert!(!output.stderr.contains("from the template"));
    assert!(!output.stderr.contains("CI notes"));
}

/// Resolved before anything is written to the ref, so a missing
/// `message_file` fails the whole update rather than showing nothing.
#[test]
fn a_migration_message_file_that_does_not_exist_fails_the_update() {
    let world = World::new();
    world.init(&[]).success();

    world.add_migration("000-note.toml", "message_file = \"NOWHERE.md\"\n");

    let output = tpl(&world.project, &["--json", "update", "--defaults"]).failure();
    assert_eq!(
        output.error_code(),
        "tpl::ops::missing_migration_message_file"
    );
}

/// Refused rather than decoded lossily, for the same reason a binary
/// `note_file` is: replacement characters would look like something was
/// shown.
#[test]
fn a_binary_migration_message_file_is_an_error() {
    let world = World::new();
    world.init(&[]).success();

    std::fs::write(
        world.template.repo.path.join("NOTE.bin"),
        [0xff, 0xfe, 0x00],
    )
    .expect("write");
    world.add_migration("000-note.toml", "message_file = \"NOTE.bin\"\n");

    let output = tpl(&world.project, &["--json", "update", "--defaults"]).failure();
    assert_eq!(
        output.error_code(),
        "tpl::ops::migration_message_file_not_utf8"
    );
}

/// Invariant 1 holds however many commits an update writes.
#[test]
fn a_migration_does_not_touch_head_the_index_or_the_worktree() {
    let world = World::new();
    world.init(&[]).success();

    let readme = world.template.repo.read("template/README.md.jinja");
    world.template.repo.remove("template/README.md.jinja");
    world
        .template
        .repo
        .write("template/docs/README.md.jinja", &readme);
    world
        .template
        .repo
        .write("template/.github/workflows/release.yml", "name: Release\n");
    world.add_migration(
        "000-move-readme.toml",
        "message = \"moved\"\n[[moves]]\nfrom = \"README.md\"\nto = \"docs/README.md\"\n",
    );

    let before = world.project.working_state();

    tpl(&world.project, &["update", "--defaults"]).success();

    let after = world.project.working_state();
    assert_eq!(before.head, after.head, "HEAD moved");
    assert_eq!(before.index, after.index, "the index changed");
    assert_eq!(before.worktree, after.worktree, "the worktree changed");
}

/// `--strict-answers` was accepted here but silently ignored — only `render`
/// enforced it. A typo'd `--answer` at `update` time must refuse the same
/// way, and before `.config/git.tpl.toml` is rewritten or the ref advanced.
#[test]
fn strict_answers_refuses_a_key_that_names_no_question() {
    let world = World::new();
    world.init(&[]).success();
    let before = world.project.rev_parse(&world.ref_name());

    let output = tpl(
        &world.project,
        &[
            "--json",
            "update",
            "--defaults",
            "--answer",
            "projct_name=oops",
            "--strict-answers",
        ],
    )
    .failure();

    assert_eq!(output.error_code(), "tpl::answers::unknown_key");
    assert_eq!(
        world.project.rev_parse(&world.ref_name()),
        before,
        "a refused update must not advance the ref"
    );
}
