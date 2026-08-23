//! `git tpl context` — what a template sees, and one expression against it.

mod common;

use common::{Template, tpl_outside};

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

    fn run(&self, args: &[&str]) -> common::Output {
        tpl_outside(self.dir.path(), self.config.path(), args)
    }
}

fn template() -> (tempfile::TempDir, Template) {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = Template::standard(dir.path());
    (dir, template)
}

/// A dump that did not match what the renderer sees would be worse than none,
/// because it would be believed. `flat` mirrors `Context::to_minijinja`.
#[test]
fn the_dump_separates_answers_computed_data_and_template() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    let json = scratch
        .run(&["--json", "context", &template.source(), "--defaults"])
        .success()
        .json();

    assert_eq!(json["answers"]["project_name"], "demo");
    assert_eq!(json["computed"]["package_name"], "demo");
    assert_eq!(json["template"]["name"], "rust-library");
    assert!(json["data"]["licenses"].is_object());

    // Answers and computed values share the top level, exactly as a template
    // body sees them; `data` and `template` stay namespaced.
    assert_eq!(json["flat"]["project_name"], "demo");
    assert_eq!(json["flat"]["package_name"], "demo");
    assert!(json["flat"].get("data").is_none());
}

/// The reason the command exists: checking a filter chain otherwise costs a
/// whole render, and the answer is buried in the output rather than stated.
#[test]
fn eval_answers_one_expression_against_the_resolved_context() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    let json = scratch
        .run(&[
            "--json",
            "context",
            &template.source(),
            "--defaults",
            "--eval",
            "{{ project_name | upper }}",
        ])
        .success()
        .json();

    assert_eq!(json["value"], "DEMO");
    assert_eq!(json["type"], "a string");
}

/// `"1"` and `1` print identically and behave differently, which is the bug
/// being debugged about half the time.
#[test]
fn eval_reports_the_type_not_just_the_rendering() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    let json = scratch
        .run(&[
            "--json",
            "context",
            &template.source(),
            "--defaults",
            "--eval",
            "{{ [1, 2, 3] | length }}",
        ])
        .success()
        .json();

    assert_eq!(json["value"], 3);
    assert_eq!(json["type"], "an integer");
}

#[test]
fn eval_reaches_a_computed_value_and_a_data_source() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    let computed = scratch
        .run(&[
            "--json",
            "context",
            &template.source(),
            "--defaults",
            "--eval",
            "{{ package_name }}",
        ])
        .success()
        .json();
    assert_eq!(computed["value"], "demo");

    let data = scratch
        .run(&[
            "--json",
            "context",
            &template.source(),
            "--defaults",
            "--eval",
            "{{ data.licenses.ids | length > 0 }}",
        ])
        .success()
        .json();
    assert_eq!(data["value"], true);
}

#[test]
fn a_bad_expression_reports_its_diagnostic_code() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    let output = scratch
        .run(&[
            "--json",
            "context",
            &template.source(),
            "--defaults",
            "--eval",
            "{{ unclosed ",
        ])
        .failure();

    assert_eq!(output.error_code(), "tpl::eval::expression");
}

#[test]
fn answers_supplied_on_the_command_line_reach_the_context() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    let json = scratch
        .run(&[
            "--json",
            "context",
            &template.source(),
            "--defaults",
            "--answer",
            "project_name=Chosen Name",
        ])
        .success()
        .json();

    assert_eq!(json["answers"]["project_name"], "Chosen Name");
    // `package_name` is `{{ project_name | lower | replace(' ', '-') }}`, so
    // this proves the computed values were resolved against the new answer
    // rather than a default.
    assert_eq!(json["computed"]["package_name"], "chosen-name");
}

#[test]
fn inspecting_a_template_needs_no_repository() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    scratch
        .run(&["--json", "context", &template.source(), "--defaults"])
        .success();

    assert!(!scratch.dir.path().join(".git").exists());
}

// ---------------------------------------------------------------------------
// The text dump.
//
// The JSON above is what a program consumes. This is the half a person reads
// while working out why a filter chain did not do what they meant, and it is
// the reason the command is worth running interactively at all.
// ---------------------------------------------------------------------------

/// The four namespaces are the whole point: `project_name` and
/// `data.licenses` are different kinds of thing, and a flat dump that mixed
/// them would not answer the question the command is asked.
#[test]
fn the_text_dump_groups_the_context_by_namespace() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    let output = scratch
        .run(&["context", &template.source(), "--defaults"])
        .success();

    output
        .says("Answers")
        .says("Computed")
        .says("Template")
        .says("Data")
        // Values are JSON-encoded, so a string is visibly a string. `demo` and
        // `"demo"` are the distinction the command exists to make.
        .says("project_name = \"demo\"")
        .says("package_name = \"demo\"")
        .says("name = \"rust-library\"")
        .says("licenses = ");
}

/// A section with nothing in it says so, rather than printing a heading and
/// then silence — which reads as a bug in the dump.
#[test]
fn an_empty_section_says_none() {
    let world = common::World::with_template(
        // No `[computed]` and no `[data]`: two of the four sections are empty.
        r#"
name = "sparse"

[questions.project_name]
type = "string"
default = "demo"
"#,
        &[("file.txt.jinja", "{{ project_name }}\n")],
    );
    let scratch = Scratch::new();

    scratch
        .run(&["context", &world.template.source(), "--defaults"])
        .success()
        .says("(none)");
}

/// The type as well as the value. `"1"` and `1` print identically and behave
/// differently, and that is the bug being debugged about half the time — so
/// the type goes to stderr and the value stays on stdout, pipeable.
#[test]
fn eval_prints_the_type_alongside_the_value() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    let output = scratch
        .run(&[
            "context",
            &template.source(),
            "--defaults",
            "--eval",
            "{{ project_name }}",
        ])
        .success();

    assert_eq!(
        output.stdout.trim(),
        "\"demo\"",
        "stdout carries the value alone, so it can be piped"
    );
    assert!(
        output.stderr.contains("(a string)"),
        "the type went missing: {:?}",
        output.stderr
    );
}

/// The same expression, evaluated to something that is not a string, proves
/// the type line is reporting rather than guessing.
#[test]
fn eval_reports_a_non_string_type() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    let output = scratch
        .run(&[
            "context",
            &template.source(),
            "--defaults",
            "--eval",
            "{{ data.licenses.ids }}",
        ])
        .success();

    assert!(
        output.stderr.contains("(an array)"),
        "expected an array type, got: {:?}",
        output.stderr
    );
    assert!(output.stdout.contains("MIT"), "{}", output.stdout);
}

/// `--strict-answers` was accepted here but silently ignored — only `render`
/// enforced it.
#[test]
fn strict_answers_refuses_a_key_that_names_no_question() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    let output = scratch
        .run(&[
            "--json",
            "context",
            &template.source(),
            "--defaults",
            "--answer",
            "projct_name=oops",
            "--strict-answers",
        ])
        .failure();

    assert_eq!(output.error_code(), "tpl::answers::unknown_key");
}
