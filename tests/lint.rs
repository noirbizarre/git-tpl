//! `git tpl lint` — the checks that need no project, no network and no render.
//!
//! Each test asserts a diagnostic **code**, never a message. The codes are the
//! stable surface; pinning prose is how error messages stop improving.

mod common;

use common::{Template, World, tpl_outside};

struct Scratch {
    dir: tempfile::TempDir,
    config: tempfile::TempDir,
}

impl Scratch {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("tempdir"),
            config: tempfile::tempdir().expect("config dir"),
        }
    }

    fn lint(&self, source: &str, extra: &[&str]) -> common::Output {
        let mut args = vec!["--json", "lint", source];
        args.extend_from_slice(extra);
        tpl_outside(self.dir.path(), self.config.path(), &args)
    }

    /// The same, without `--json` — the report a person reads.
    ///
    /// Prose is asserted here and nowhere else, and only the parts that are a
    /// contract: the code, the path and the counts. The wording of a message
    /// stays free to improve.
    fn lint_text(&self, source: &str, extra: &[&str]) -> common::Output {
        let mut args = vec!["lint", source];
        args.extend_from_slice(extra);
        tpl_outside(self.dir.path(), self.config.path(), &args)
    }
}

/// Every diagnostic code reported, in order.
fn codes(output: &common::Output) -> Vec<String> {
    output.json()["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .map(|d| d["code"].as_str().expect("code").to_string())
        .collect()
}

#[test]
fn a_sound_template_reports_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = Template::standard(dir.path());
    let scratch = Scratch::new();

    let output = scratch.lint(&template.source(), &[]).success();
    assert!(codes(&output).is_empty(), "{:?}", codes(&output));
}

#[test]
fn linting_needs_no_repository_of_its_own() {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = Template::standard(dir.path());
    let scratch = Scratch::new();

    scratch.lint(&template.source(), &[]).success();
    assert!(!scratch.dir.path().join(".git").exists());
}

/// The failure that motivated the command. One gated file, so there is no
/// collision to catch it: the renderer would write a file called `.yaml` and
/// say nothing.
#[test]
fn a_conditional_segment_that_leaves_its_suffix_outside_is_an_error() {
    let world = World::with_template(
        r#"
name = "gated"

[questions.msrv]
type = "boolean"
default = true
"#,
        &[(
            ".github/workflows/{% if msrv %}msrv{% endif %}.yaml",
            "name: msrv\n",
        )],
    );
    let scratch = Scratch::new();

    let output = scratch.lint(&world.template.source(), &[]).failure();
    assert_eq!(codes(&output), ["tpl::lint::degenerate_path"]);
    assert_eq!(output.json()["errors"], 1);
}

/// The same shape written correctly. A check that flagged this would be
/// unusable, because it is the only way to make a whole file conditional.
#[test]
fn the_correct_conditional_form_is_not_flagged() {
    let world = World::with_template(
        r#"
name = "gated"

[questions.msrv]
type = "boolean"
default = true

[questions.docs]
type = "boolean"
default = true
"#,
        &[
            (
                ".github/workflows/{% if msrv %}msrv.yaml{% endif %}",
                "name: msrv\n",
            ),
            // A `.jinja` file keeps its suffix outside the block, because the
            // suffix is stripped before the segments are rendered.
            (
                "{% if docs %}zensical.toml{% endif %}.jinja",
                "name = \"{{ template.name }}\"\n",
            ),
            ("{% if docs %}docs{% endif %}/index.md", "# docs\n"),
        ],
    );
    let scratch = Scratch::new();

    let output = scratch.lint(&world.template.source(), &[]).success();
    assert!(codes(&output).is_empty(), "{:?}", codes(&output));
}

/// A syntax error in a branch no answer set reaches is still a syntax error.
/// Rendering only ever proves the branch it took.
#[test]
fn a_syntax_error_is_found_without_rendering() {
    let world = World::with_template(
        r#"name = "broken""#,
        &[("file.txt.jinja", "{% if true %}unterminated\n")],
    );
    let scratch = Scratch::new();

    let output = scratch.lint(&world.template.source(), &[]).failure();
    assert_eq!(codes(&output), ["tpl::lint::syntax"]);
}

/// `${{ }}` is inside MiniJinja's syntax. It renders to `$`, the YAML stays
/// valid, and nothing fails until the workflow runs.
#[test]
fn a_github_expression_in_a_rendered_file_is_a_warning() {
    let world = World::with_template(
        r#"name = "leaky""#,
        &[("ci.yaml.jinja", "runs-on: ${{ matrix.os }}\n")],
    );
    let scratch = Scratch::new();

    // A warning, not an error: a template may legitimately mean it, and a lint
    // that fails on warnings is a lint people stop running.
    let output = scratch.lint(&world.template.source(), &[]).success();
    assert_eq!(codes(&output), ["tpl::lint::foreign_expression"]);
    assert_eq!(output.json()["warnings"], 1);
}

#[test]
fn a_raw_block_makes_a_github_expression_fine() {
    let world = World::with_template(
        r#"name = "escaped""#,
        &[(
            "ci.yaml.jinja",
            "{% raw %}runs-on: ${{ matrix.os }}{% endraw %}\n",
        )],
    );
    let scratch = Scratch::new();

    let output = scratch.lint(&world.template.source(), &[]).success();
    assert!(codes(&output).is_empty(), "{:?}", codes(&output));
}

/// A file not named `.jinja` is copied byte-for-byte, so its `${{ }}` is never
/// at risk and reporting it would be noise.
#[test]
fn a_verbatim_file_is_not_checked_for_expressions() {
    let world = World::with_template(
        r#"name = "verbatim""#,
        &[("ci.yaml", "runs-on: ${{ matrix.os }}\n")],
    );
    let scratch = Scratch::new();

    let output = scratch.lint(&world.template.source(), &[]).success();
    assert!(codes(&output).is_empty(), "{:?}", codes(&output));
}

/// The renderer catches this only for the answer set it was given. Here it is
/// structural: both paths collapse to the same literal, so *some* answer set
/// will collide, and finding out which is not the author's job.
#[test]
fn two_conditional_paths_collapsing_to_one_name_collide() {
    let world = World::with_template(
        r#"
name = "colliding"

[questions.msrv]
type = "boolean"
default = true

[questions.docs]
type = "boolean"
default = true
"#,
        &[
            (
                ".github/workflows/{% if msrv %}msrv{% endif %}.yaml",
                "name: msrv\n",
            ),
            (
                ".github/workflows/{% if docs %}docs{% endif %}.yaml",
                "name: docs\n",
            ),
        ],
    );
    let scratch = Scratch::new();

    let output = scratch.lint(&world.template.source(), &[]).failure();
    let found = codes(&output);
    assert!(
        found.contains(&"tpl::lint::collision".to_string()),
        "{found:?}"
    );
}

/// An unknown name in a manifest expression already fails the graph build.
/// Lint surfaces it without a project, which is the new part.
#[test]
fn an_unknown_reference_in_the_manifest_fails_the_lint() {
    let world = World::with_template(
        r#"
name = "typo"

[questions.project_name]
type = "string"
default = "demo"

[computed]
slug = "{{ projct_name }}"
"#,
        &[("file.txt.jinja", "{{ slug }}\n")],
    );
    let scratch = Scratch::new();

    let output = scratch.lint(&world.template.source(), &[]).failure();
    assert_eq!(output.error_code(), "tpl::graph::unknown_reference");
}

/// The edit-and-see loop: an author must be able to check a change before
/// committing it, or the check is one they run after the mistake is history.
#[test]
fn dirty_lints_the_uncommitted_template() {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = Template::standard(dir.path());
    let scratch = Scratch::new();

    template
        .repo
        .write("template/broken.txt.jinja", "{% if true %}unterminated\n");

    scratch.lint(&template.source(), &[]).success();
    let output = scratch.lint(&template.source(), &["--dirty"]).failure();
    assert_eq!(codes(&output), ["tpl::lint::syntax"]);
}

// ---------------------------------------------------------------------------
// The text report.
//
// Everything above goes through `--json`, which is the right default for a
// test: codes are stable, prose is not. But the report a person actually reads
// is a different code path, and until these existed it had never run.
// ---------------------------------------------------------------------------

#[test]
fn a_sound_template_says_it_found_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = Template::standard(dir.path());
    let scratch = Scratch::new();

    scratch
        .lint_text(&template.source(), &[])
        .success()
        .says("No problems found.");
}

/// A finding is only actionable if it says which file, what the rule was, and
/// what to do — so the report is asserted to carry all three, plus the code a
/// reader will grep the diagnostics reference for.
#[test]
fn the_text_report_names_the_code_the_path_and_the_help() {
    let world = World::with_template(
        r#"
name = "gated"

[questions.msrv]
type = "boolean"
default = true
"#,
        &[(
            ".github/workflows/{% if msrv %}msrv{% endif %}.yaml",
            "name: msrv\n",
        )],
    );
    let scratch = Scratch::new();

    let output = scratch.lint_text(&world.template.source(), &[]).failure();

    output
        .says("error[tpl::lint::degenerate_path]")
        .says(".github/workflows/")
        .says("help:")
        .says("1 error(s), 0 warning(s)");
}

/// Warnings are counted separately and, on their own, are not a failure: a
/// lint that fails on them is a lint people stop running.
#[test]
fn the_text_report_counts_warnings_apart_from_errors() {
    let world = World::with_template(
        r#"name = "leaky""#,
        &[("ci.yaml.jinja", "runs-on: ${{ matrix.os }}\n")],
    );
    let scratch = Scratch::new();

    scratch
        .lint_text(&world.template.source(), &[])
        .success()
        .says("warning[tpl::lint::foreign_expression]")
        .says("0 error(s), 1 warning(s)");
}

/// A binary file that happens to end in `.jinja` is copied, not rendered, so
/// parsing it would report a syntax error in something that never runs. A PNG
/// named `logo.png.jinja` is a mistake, but it is not a syntax error.
#[test]
fn a_binary_jinja_file_is_not_parsed() {
    let world = World::with_template(
        r#"name = "binary""#,
        &[(
            // NUL bytes in the first 8 KiB are the binary test, and the rest
            // would be a syntax error if it were ever parsed.
            "logo.png.jinja",
            "\u{0}\u{0}\u{0}PNG{% if unterminated\u{0}",
        )],
    );
    let scratch = Scratch::new();

    let output = scratch.lint(&world.template.source(), &[]).success();
    assert!(
        codes(&output).is_empty(),
        "a binary file was parsed as a template: {:?}",
        codes(&output)
    );
}

/// An unbalanced `{% if %}` in a *path* is not a conditional segment at all.
/// The degenerate-path check has to decline to analyse it rather than guess at
/// what it collapses to — the syntax check is what reports it.
#[test]
fn an_unbalanced_conditional_in_a_path_is_not_analysed_as_one() {
    let world = World::with_template(
        r#"
name = "unbalanced"

[questions.msrv]
type = "boolean"
default = true
"#,
        &[("{% if msrv %}never-closed.yaml", "name: x\n")],
    );
    let scratch = Scratch::new();

    // Whatever else is reported, it must not be a degenerate path: the segment
    // was never established to be conditional.
    let output = scratch.lint(&world.template.source(), &[]);
    assert!(
        !codes(&output).contains(&"tpl::lint::degenerate_path".to_string()),
        "an unbalanced conditional was analysed as one: {:?}",
        codes(&output)
    );
}
