//! Snapshots of what the commands actually print.
//!
//! The other test files pin *behaviour*: an exit code, a ref that moved, a key
//! a script has hard-coded. This one pins *layout* — the human transcripts
//! `docs/usage/` shows a reader, and the envelopes `docs/reference/json.md`
//! calls a contract. Neither had a whole-output test, so a change to a
//! `theme.rs` helper could rewrite a documented transcript and ship.
//!
//! A failure here is therefore not necessarily a bug. It means output the
//! documentation quotes has changed, and the matching page needs the same edit
//! in the same pull request. Review with `mise run snapshots`.
//!
//! Only layout belongs here. Assertions about *values* — that a count is right,
//! that a ref advanced — stay in the per-command files, where a failure names
//! the thing that broke instead of showing a diff and leaving the reader to
//! work out which line mattered.

mod common;

use common::{Output, World, redact_paths, redact_roots, snapshot_settings, tpl};

/// Snapshot a run under this world's redactions.
///
/// Every caller goes through here rather than reaching for `insta` directly:
/// the redactions are per-world — the temporary directory's name is different
/// on every run — so a snapshot taken without them passes once and never again.
fn snapshot(name: &str, world: &World, output: &Output) {
    let transcript = redact_paths(world, &output.transcript());
    snapshot_settings().bind(|| insta::assert_snapshot!(name, transcript));
}

/// A world whose template has moved, so there is something to report.
fn pending() -> World {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    world
}

// --- the redaction itself ---------------------------------------------------
//
// Every one of these cases is a snapshot that passed on Linux and failed on
// another platform. They are asserted here so that the next such bug is found
// by the developer who wrote it rather than by CI.

/// macOS resolves the temporary directory through `/private`.
///
/// The two spellings share a suffix, so redacting the shorter one first hits
/// the middle of the longer one and leaves `/private[tmp]` behind.
#[test]
fn a_path_is_redacted_at_its_longest_spelling() {
    let roots = vec![
        "/private/var/folders/x/T/.tmp7Nd".to_string(),
        "/var/folders/x/T/.tmp7Nd".to_string(),
    ];

    let redacted = redact_roots(
        &roots,
        "found at `/private/var/folders/x/T/.tmp7Nd/project`",
    );

    assert_eq!(redacted, "found at `[tmp]/project`");
}

/// Windows prints `\`, and a JSON string escapes it to `\\`.
///
/// Both become `/`, so one snapshot serves every platform.
#[test]
fn windows_separators_are_rewritten_not_merely_matched() {
    let roots = vec![r"C:\Users\RUNNER~1\Temp\.tmpoT".to_string()];

    assert_eq!(
        redact_roots(&roots, r"Template:  C:\Users\RUNNER~1\Temp\.tmpoT\template"),
        "Template:  [tmp]/template"
    );
    assert_eq!(
        redact_roots(
            &roots,
            r#""template": "C:\\Users\\RUNNER~1\\Temp\\.tmpoT\\template""#
        ),
        r#""template": "[tmp]/template""#
    );
}

/// A redaction stops at the delimiter, rather than eating the rest of the line.
#[test]
fn a_redaction_stops_where_the_path_does() {
    let roots = vec!["/tmp/.tmp7Nd".to_string()];

    let redacted = redact_roots(
        &roots,
        "at `/tmp/.tmp7Nd/project/.config/git.tpl.toml`, sorry",
    );

    assert_eq!(redacted, "at `[tmp]/project/.config/git.tpl.toml`, sorry");
}

// --- human output -----------------------------------------------------------

/// `docs/usage/status.md` — before anything is rendered.
#[test]
fn status_says_when_nothing_has_been_rendered_yet() {
    let world = World::new();
    world.project.write(
        ".config/git.tpl.toml",
        &format!(
            "[template]\nsource = \"{}\"\n",
            world.template.source().replace('\\', "/")
        ),
    );
    world.project.commit_all("chore: declare the template");

    let output = tpl(&world.project, &["status"]);

    snapshot("status_nothing_rendered", &world, &output);
}

/// `docs/usage/status.md:9` — the field block, and its alignment.
#[test]
fn status_reports_the_template_the_revision_and_the_worktree() {
    let world = World::new();
    world.init(&[]).success();

    let output = tpl(&world.project, &["status"]).success();

    snapshot("status_up_to_date", &world, &output);
}

/// `docs/usage/status.md` — the pending form, including the closing advice.
///
/// Exit 2 is the documented "pending" code, and it is asserted rather than left
/// to the snapshot so that a wrong code fails by name.
#[test]
fn status_reports_a_moved_template_as_pending() {
    let world = pending();

    let output = tpl(&world.project, &["status"]).code(2);

    snapshot("status_pending", &world, &output);
}

/// `docs/usage/diff.md:13` — the change lines and the totals line.
#[test]
fn diff_stat_summarises_what_a_merge_would_change() {
    let world = pending();
    tpl(&world.project, &["update", "--defaults"]).success();

    let output = tpl(&world.project, &["diff", "--stat"]).success();

    snapshot("diff_stat", &world, &output);
}

/// `docs/usage/update.md:57` — the headline, the change list and the advice.
#[test]
fn update_reports_what_changed() {
    let world = pending();

    let output = tpl(&world.project, &["update", "--defaults"]).success();

    snapshot("update_changed", &world, &output);
}

/// `docs/usage/update.md:81` — the one-line no-op.
#[test]
fn update_says_when_it_is_already_up_to_date() {
    let world = World::new();
    world.init(&[]).success();

    let output = tpl(&world.project, &["update", "--defaults"]).success();

    snapshot("update_up_to_date", &world, &output);
}

/// `docs/usage/init.md:65` — everything `init` says about a fresh attachment.
#[test]
fn init_reports_what_it_created() {
    let world = World::new();

    let output = world.init(&[]).success();

    snapshot("init_created", &world, &output);
}

/// `docs/usage/backport.md` — the summary, and the `git am` line it ends on.
///
/// The whole transcript, patch included, because that page's claim is that the
/// last line is exactly what you run next. If the summary and the patch ever
/// swap streams, the documented pipe stops working and this is what says so.
#[test]
fn backport_reports_the_patch_and_how_to_apply_it() {
    let world = World::new();
    world.init(&[]).success();
    // A file the standard template copies byte-for-byte: the easy path, and
    // the one the page opens with.
    world.project.write(
        "ci.yml",
        "name: CI\non: [push, pull_request]\njobs:\n  test:\n    steps:\n      - run: echo ${{ github.sha }}\n",
    );

    let output = tpl(&world.project, &["backport"]).success();

    snapshot("backport_patch", &world, &output);
}

/// `docs/usage/backport.md` — the refusal a user meets most.
#[test]
fn backport_refuses_a_change_to_a_substituted_line() {
    let world = World::new();
    world.init(&[]).success();
    // The heading comes from `project_name`, so this is a changed *answer*,
    // not a change to the template.
    world
        .project
        .write("README.md", "# renamed\n\nLicensed under MIT License.\n");

    let output = tpl(&world.project, &["backport"]).failure();

    snapshot("backport_substituted", &world, &output);
}

// --- JSON envelopes ---------------------------------------------------------

/// `docs/reference/json.md` — the `status` payload.
///
/// Pretty-printed by the harness for review; `tests/json.rs` still pins that
/// the wire form is a single compact object.
#[test]
fn status_json_envelope() {
    let world = pending();

    let output = tpl(&world.project, &["--json", "status"]).code(2);

    snapshot("json_status", &world, &output);
}

/// `docs/reference/json.md` — the `diff` payload, with its per-file stat.
#[test]
fn diff_json_envelope() {
    let world = pending();
    tpl(&world.project, &["update", "--defaults"]).success();

    let output = tpl(&world.project, &["--json", "diff", "--stat"]).success();

    snapshot("json_diff", &world, &output);
}

/// `docs/reference/json.md` — the `update` payload.
#[test]
fn update_json_envelope() {
    let world = pending();

    let output = tpl(&world.project, &["--json", "update", "--defaults"]).success();

    snapshot("json_update", &world, &output);
}

/// `docs/reference/json.md` — the `init` payload, including the merge result.
#[test]
fn init_json_envelope() {
    let world = World::new();

    let output = world.init(&["--json"]).success();

    snapshot("json_init", &world, &output);
}

/// `docs/reference/json.md` — the failure envelope.
///
/// `tests/update.rs` proves the behaviour of running without a configuration;
/// what is pinned here is the shape `report::error` wraps it in, which is what
/// a caller branches on. The message is inside the snapshot and will move; the
/// point of review is to notice that `ok`, `error.code` and `error.help` did
/// not.
#[test]
fn a_failure_envelope_has_a_code_and_a_help() {
    let world = World::new();

    let output = tpl(&world.project, &["--json", "update"]).failure();

    snapshot("json_failure", &world, &output);
}

/// `docs/reference/json.md` — the `backport` payload, patch and all.
///
/// The `patch` key is the whole point of this one: it pins that the mailbox
/// travels *inside* the envelope, because stdout under `--json` is one object.
#[test]
fn backport_json_envelope() {
    let world = World::new();
    world.init(&[]).success();
    world.project.write(
        "ci.yml",
        "name: CI\non: [push, pull_request]\njobs:\n  test:\n    steps:\n      - run: echo ${{ github.sha }}\n",
    );

    let output = tpl(&world.project, &["--json", "backport"]).success();

    snapshot("json_backport", &world, &output);
}
