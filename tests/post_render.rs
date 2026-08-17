//! ADR-019: a template may address the user and declare Git remotes, and still
//! runs nothing.
//!
//! Real repositories throughout. The premise of the project is that Git's
//! behaviour is the behaviour, so a remote is asserted by asking Git for it.

mod common;

use common::{World, tpl, tpl_colored};

/// A template that declares one remote and one inline note.
///
/// Top-level keys come before every table, as TOML requires. This is also why
/// the key is `note` and not `message`: a `message =` after `[questions.org]`
/// would be that *question's* `message`, silently.
const MANIFEST: &str = r#"
name = "demo"
note = "Next: run {{ 'scripts/bootstrap.sh' }} in {{ org }}"

[remotes]
origin = "https://example.invalid/{{ org }}/demo.git"

[questions.org]
type = "string"
default = "acme"
"#;

fn world() -> World {
    World::with_template(MANIFEST, &[("README.md", "# demo\n")])
}

/// `--json`, so the payload can be asserted as well as the prose.
fn init_json(world: &World, extra: &[&str]) -> common::Output {
    let source = world.template.source();
    let mut args = vec!["--json", "init", source.as_str(), "--defaults"];
    args.extend_from_slice(extra);
    tpl(&world.project, &args)
}

// --- remotes ----------------------------------------------------------------

#[test]
fn a_declared_remote_is_added_on_init() {
    let world = world();
    world.init(&[]).success();

    assert_eq!(
        world.project.git(&["remote", "get-url", "origin"]),
        "https://example.invalid/acme/demo.git"
    );
}

/// The URL is a fact about the template's conventions, derived from the
/// answers — which is the only reason a template gets to declare one at all.
#[test]
fn a_remote_url_is_rendered_from_the_answers() {
    let world = world();
    world.init(&["--answer", "org=widgets"]).success();

    assert_eq!(
        world.project.git(&["remote", "get-url", "origin"]),
        "https://example.invalid/widgets/demo.git"
    );
}

/// The URL in the repository was put there by a person. A template that could
/// repoint it is a template that could redirect the user's next push.
#[test]
fn an_existing_remote_with_a_different_url_is_left_alone() {
    let world = world();
    world
        .project
        .git(&["remote", "add", "origin", "git@github.com:someone/real.git"]);

    let output = world.init(&[]).success();

    assert_eq!(
        world.project.git(&["remote", "get-url", "origin"]),
        "git@github.com:someone/real.git",
        "the user's own remote must survive"
    );
    // Both URLs, because the user cannot choose between them without seeing
    // them together.
    assert!(output.stderr.contains("git@github.com:someone/real.git"));
    assert!(
        output
            .stderr
            .contains("https://example.invalid/acme/demo.git")
    );
}

/// The refusal has to reach a caller that is not reading prose, and `existing`
/// is the field that lets one tell a refusal from a no-op.
#[test]
fn a_skipped_remote_is_reported_in_the_json_payload() {
    let world = world();
    world
        .project
        .git(&["remote", "add", "origin", "git@github.com:someone/real.git"]);

    let output = init_json(&world, &[]).success();
    let remote = output.json()["remotes"][0].clone();

    assert_eq!(remote["status"], "skipped");
    assert_eq!(remote["existing"], "git@github.com:someone/real.git");
    // What the template asked for, even though it did not get it — the case a
    // caller most needs it in.
    assert_eq!(remote["url"], "https://example.invalid/acme/demo.git");
}

/// Reported as unchanged rather than added, so a re-init does not claim to have
/// done something it did not.
#[test]
fn an_existing_remote_with_the_same_url_is_not_reported_as_changed() {
    let world = world();
    world.project.git(&[
        "remote",
        "add",
        "origin",
        "https://example.invalid/acme/demo.git",
    ]);

    let output = init_json(&world, &[]).success();
    let remotes = output.json()["remotes"].clone();

    assert_eq!(remotes[0]["status"], "unchanged");
    assert!(remotes[0]["existing"].is_null());
}

/// "Nothing happened" on every re-init is the kind of line that trains people
/// to stop reading them. Asserted, because otherwise an accidental `say` in
/// that arm would pass every other test here.
#[test]
fn an_unchanged_remote_prints_nothing() {
    let world = world();
    world.project.git(&[
        "remote",
        "add",
        "origin",
        "https://example.invalid/acme/demo.git",
    ]);

    let output = world.init(&[]).success();

    assert!(
        !output.stderr.contains("origin"),
        "an unchanged remote must be silent, got:\n{}",
        output.stderr
    );
}

/// `update` being a ref-only operation is most of its value. A template that
/// could add a remote on every update could add one long after the user had
/// removed it.
#[test]
fn update_does_not_add_remotes() {
    let world = world();
    world.init(&[]).success();
    world.project.git(&["remote", "remove", "origin"]);

    world.template.repo.write("template/EXTRA.md", "extra\n");
    world.template.repo.commit_all("feat: add a file");

    tpl(&world.project, &["update"]).success();

    assert_eq!(
        world.project.git(&["remote"]),
        "",
        "update must not add a remote"
    );
}

/// ADR-019's closure rule admits adding a remote and nothing beyond it.
#[test]
fn a_declared_remote_is_neither_fetched_nor_pushed() {
    // The URL is unroutable. If `init` fetched or pushed, it would hang or
    // fail; that it succeeds promptly is half the assertion.
    let world = world();
    world.init(&[]).success();

    let refs = world
        .project
        .git(&["for-each-ref", "--format=%(refname)", "refs/remotes"]);
    assert_eq!(refs, "", "nothing was fetched");
}

#[test]
fn a_remote_appears_in_the_json_payload() {
    let world = world();
    let output = init_json(&world, &[]).success();

    let remote = output.json()["remotes"][0].clone();
    assert_eq!(remote["name"], "origin");
    assert_eq!(remote["url"], "https://example.invalid/acme/demo.git");
    assert_eq!(remote["status"], "added");
}

// --- the note ---------------------------------------------------------------

/// The one thing a template could not previously do. Without it the status quo
/// — render `bootstrap.sh` and print a line about it — had nowhere for the line
/// to come from.
#[test]
fn a_note_is_printed_after_the_merge() {
    let world = world();
    let output = world.init(&[]).success();

    assert!(
        output
            .stderr
            .contains("Next: run scripts/bootstrap.sh in acme"),
        "stderr was:\n{}",
        output.stderr
    );
}

/// The frame is what keeps a note from *claiming* to be git-tpl.
#[test]
fn a_note_is_attributed_to_the_template() {
    let world = world();
    let output = world.init(&[]).success();

    assert!(
        output.stderr.contains("from the template"),
        "stderr was:\n{}",
        output.stderr
    );
}

/// A world whose template carries a note file beside the manifest, outside the
/// render root — the same place a partial lives.
fn world_with_note_file(declared: &str, path: &str, content: &str) -> World {
    World::with_shared_template(
        &format!(
            r#"
            name = "demo"
            note_file = "{declared}"

            [questions.project]
            type = "string"
            default = "demo"
            "#
        ),
        &[("README.md", "# demo\n")],
        &[(path, content)],
    )
}

/// The point of the redesign: a note is guidance read from the template, not a
/// file the project has to carry.
#[test]
fn a_note_file_is_not_rendered_into_the_project() {
    let world = world_with_note_file("NEXT-STEPS.md", "NEXT-STEPS.md", "Run bootstrap.\n");
    let output = world.init(&[]).success();

    assert!(output.stderr.contains("Run bootstrap."));
    assert!(
        !world.project.exists("NEXT-STEPS.md"),
        "the note must not be written into the project"
    );
}

/// The same rule the renderer applies to files, and nothing is inferred from
/// the content: an author who wants interpolation names the `.jinja`.
#[test]
fn a_jinja_note_file_is_rendered() {
    let world = world_with_note_file(
        "NEXT-STEPS.md.jinja",
        "NEXT-STEPS.md.jinja",
        "Set up {{ project }} by running bootstrap.\n",
    );
    let output = world.init(&[]).success();

    assert!(
        output.stderr.contains("Set up demo by running bootstrap."),
        "stderr was:\n{}",
        output.stderr
    );
}

#[test]
fn a_plain_note_file_is_shown_verbatim() {
    let world = world_with_note_file("NEXT-STEPS.md", "NEXT-STEPS.md", "Braces {{ stay }}.\n");
    let output = world.init(&[]).success();

    assert!(
        output.stderr.contains("Braces {{ stay }}."),
        "a note without a .jinja suffix is not a template; stderr was:\n{}",
        output.stderr
    );
}

/// Repository-root-relative, so a path may point inside the render root. It
/// reads the template source there, and the file is also rendered into the
/// project — harmless, and cheaper to document than to police.
#[test]
fn a_note_file_may_point_inside_the_render_root() {
    let world = World::with_template(
        r#"
        name = "demo"
        note_file = "template/INSIDE.md"
        "#,
        &[("INSIDE.md", "Both a file and a note.\n")],
    );

    let output = world.init(&[]).success();

    assert!(output.stderr.contains("Both a file and a note."));
    assert_eq!(world.project.read("INSIDE.md"), "Both a file and a note.\n");
}

/// A template may choose to say nothing for these answers. That is a decision,
/// not a mistake, and is the one absence not worth reporting.
#[test]
fn a_note_file_path_that_renders_empty_shows_no_note() {
    let world = World::with_shared_template(
        r#"
        name = "demo"
        note_file = "{% if ci %}CI.md{% endif %}"

        [questions.ci]
        type = "boolean"
        default = false
        "#,
        &[("README.md", "# demo\n")],
        &[("CI.md", "CI notes.\n")],
    );

    let output = world.init(&[]).success();
    assert!(!output.stderr.contains("from the template"));
    assert!(!output.stderr.contains("CI notes."));
}

#[test]
fn a_note_file_path_may_be_an_expression() {
    let world = World::with_shared_template(
        r#"
        name = "demo"
        note_file = "notes/{{ language }}.md"

        [questions.language]
        type = "string"
        default = "rust"
        "#,
        &[("README.md", "# demo\n")],
        &[
            ("notes/rust.md", "Cargo build.\n"),
            ("notes/python.md", "uv sync.\n"),
        ],
    );

    assert!(world.init(&[]).success().stderr.contains("Cargo build."));

    let other = World::with_shared_template(
        r#"
        name = "demo"
        note_file = "notes/{{ language }}.md"

        [questions.language]
        type = "string"
        default = "rust"
        "#,
        &[("README.md", "# demo\n")],
        &[
            ("notes/rust.md", "Cargo build.\n"),
            ("notes/python.md", "uv sync.\n"),
        ],
    );
    assert!(
        other
            .init(&["--answer", "language=python"])
            .success()
            .stderr
            .contains("uv sync.")
    );
}

/// The reason the note is resolved before the ref is created: failing is free
/// there, and nothing the user has to undo survives it.
#[test]
fn a_note_file_that_does_not_exist_fails_before_anything_is_written() {
    let world = World::with_template(
        "name = \"demo\"\nnote_file = \"ABSENT.md\"",
        &[("README.md", "# demo\n")],
    );

    let output = world.init(&[]).failure();
    assert!(
        output.stderr.contains("tpl::ops::missing_note_file"),
        "stderr was:\n{}",
        output.stderr
    );

    assert!(
        !world.project.has_ref(&world.ref_name()),
        "no ref was created"
    );
    assert!(!world.project.exists(".config/git.tpl.toml"), "no config");
    assert!(!world.project.exists("README.md"), "nothing was merged");
    assert_eq!(world.project.status(), "", "the worktree is untouched");
}

/// The trap the rule exists for: `note_file` is repository-root relative, so
/// the render-root path names nothing.
#[test]
fn a_render_root_path_is_reported_as_missing() {
    let world = world_with_note_file("template/NEXT.md", "NEXT.md", "hi\n");
    let output = world.init(&[]).failure();

    assert!(output.stderr.contains("tpl::ops::missing_note_file"));
    // The help has to name the trap, or the diagnostic restates itself.
    assert!(
        output.stderr.contains("repository root"),
        "stderr was:\n{}",
        output.stderr
    );
}

/// Refused rather than decoded lossily: replacement characters would look like
/// something was shown.
#[test]
fn a_binary_note_file_is_an_error() {
    let world = World::with_template(
        "name = \"demo\"\nnote_file = \"NOTE.bin\"",
        &[("README.md", "# demo\n")],
    );
    std::fs::write(
        world.template.repo.path.join("NOTE.bin"),
        [0xff, 0xfe, 0x00],
    )
    .expect("write");
    world.template.repo.commit_all("feat: a binary note");

    let output = world.init(&[]).failure();
    assert!(
        output.stderr.contains("tpl::ops::note_file_not_utf8"),
        "stderr was:\n{}",
        output.stderr
    );
}

/// Caught at load time, before a single question is asked.
#[test]
fn declaring_both_note_forms_is_a_manifest_error() {
    let world = World::with_template(
        "name = \"demo\"\nnote = \"hi\"\nnote_file = \"N.md\"",
        &[("README.md", "# demo\n")],
    );

    let output = world.init(&[]).failure();
    assert!(
        output.stderr.contains("mutually exclusive"),
        "stderr was:\n{}",
        output.stderr
    );
}

/// A log file is no more readable for containing escape sequences, and a
/// `--json` consumer is not a terminal at all.
#[test]
fn a_note_carries_no_escape_sequences_when_output_is_piped() {
    let world = World::with_template(
        // A bold sequence and a screen-clear, written by the template.
        "name = \"demo\"\nnote = \"\\u001b[1mbold\\u001b[0m and \\u001b[2Jgone\"",
        &[("README.md", "# demo\n")],
    );

    let output = world.init(&[]).success();

    assert!(
        !output.stderr.contains('\x1b'),
        "stderr must be plain when piped:\n{:?}",
        output.stderr
    );
    assert!(output.stderr.contains("bold and gone"));
}

/// The other leg of that choice. Without this the ternary selecting
/// `Formatting::Allowed` could be inverted and every other test would pass,
/// because the harness forces `--color never`.
#[test]
fn a_note_keeps_its_colour_on_a_terminal() {
    let world = World::with_template(
        "name = \"demo\"\nnote = \"\\u001b[1mbold\\u001b[0m and \\u001b[2Jgone\"",
        &[("README.md", "# demo\n")],
    );

    let output = tpl_colored(
        &world.project,
        &["init", &world.template.source(), "--defaults"],
    )
    .success();

    assert!(
        output.stderr.contains("\x1b[1m"),
        "styling must survive on a terminal:\n{:?}",
        output.stderr
    );
    // ...but the screen-clear still does not.
    assert!(!output.stderr.contains("\x1b[2J"));
    assert!(output.stderr.contains("bold"));
}

/// A note that is nothing but a stripped sequence has nothing to show, and an
/// empty frame reads as the template having trailed off.
#[test]
fn a_note_that_sanitises_to_nothing_prints_no_block() {
    let world = World::with_template(
        "name = \"demo\"\nnote = \"\\u001b[2J\\u001b[3A\"",
        &[("README.md", "# demo\n")],
    );

    let output = world.init(&[]).success();
    assert!(
        !output.stderr.contains("from the template"),
        "stderr was:\n{}",
        output.stderr
    );
}

/// Raw on stdout, because escape sequences are a terminal's problem and this
/// stream reaches no terminal.
#[test]
fn the_json_payload_carries_the_note_unsanitised() {
    let world = world();
    let output = init_json(&world, &[]).success();

    assert_eq!(
        output.json()["note"],
        "Next: run scripts/bootstrap.sh in acme"
    );
}

/// A template with neither must gain nothing — the keys are additive, and an
/// empty block or a `[]` would change every existing template's output.
#[test]
fn a_template_that_declares_neither_reports_neither() {
    let world = World::new();
    let output = init_json(&world, &[]).success();

    let payload = output.json();
    assert!(payload["note"].is_null());
    assert_eq!(payload["remotes"], serde_json::json!([]));
    assert!(!output.stderr.contains("from the template"));
}

/// Invariant 5 is untouched. git-tpl runs nothing a note names, so a note
/// saying "run this" is exactly as dangerous as a README saying it.
#[test]
fn a_note_naming_a_command_does_not_run_it() {
    let world = World::with_template(
        "name = \"demo\"\nnote = \"run: touch PWNED\"",
        &[("README.md", "# demo\n")],
    );

    world.init(&[]).success();

    assert!(
        !world.project.exists("PWNED"),
        "a note must never be executed"
    );
}

// --- lint -------------------------------------------------------------------

/// `lint` reports the mistake without a repository, which is the only place a
/// template author will see it before a user does.
#[test]
fn lint_reports_a_note_file_that_the_template_does_not_contain() {
    let world = world_with_note_file("template/NEXT.md", "NEXT.md", "hi\n");
    let output = tpl(&world.project, &["lint", &world.template.source()]).failure();

    assert!(
        output.stderr.contains("tpl::lint::missing_note_file"),
        "stderr was:\n{}",
        output.stderr
    );
}

#[test]
fn lint_accepts_a_note_file_the_template_contains() {
    let world = world_with_note_file("NEXT.md", "NEXT.md", "hi\n");
    tpl(&world.project, &["lint", &world.template.source()]).success();
}

/// A rule nobody can deny is a rule that cannot be enforced in CI.
#[test]
fn the_note_file_rule_can_be_denied_by_code() {
    let world = world_with_note_file("NEXT.md", "NEXT.md", "hi\n");
    tpl(
        &world.project,
        &[
            "lint",
            &world.template.source(),
            "--deny",
            "tpl::lint::missing_note_file",
        ],
    )
    .success();
}
