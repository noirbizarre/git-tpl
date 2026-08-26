//! `git tpl test`'s `[commands]` — ADR-027.
//!
//! Real processes throughout, on the same principle as the rest of the
//! suite: a mock subprocess would test the mock. Every command here is a
//! `coreutils` binary (`ls`, `mkdir`, `touch`) present on every Linux and
//! macOS runner this project ships from, which is why the file is gated to
//! `unix` rather than taught to shell out through `cmd.exe` as well.

#![cfg(unix)]

mod common;

use std::path::Path;

use common::{Repo, Template, tpl_outside};

/// A template with one file, `marker.txt`, so a case's `commands.rendered`/
/// `commands.after` have something in the sandbox that came from the render
/// itself, distinct from anything `commands.before` seeded.
fn template(parent: &Path, cases: &[(&str, &str)]) -> Template {
    let built = Template::minimal(parent, "name = \"cmdtest\"\n", &[("marker.txt", "hello\n")]);
    for (path, body) in cases {
        built.repo.write(path, body);
    }
    if !cases.is_empty() {
        built.repo.commit_all("test: cases");
    }
    built
}

/// Run `git tpl test` from outside any repository, against a template path.
fn run(template: &Template, args: &[&str]) -> common::Output {
    let mut all = vec!["--json", "test", "__TEMPLATE__"];
    all.extend_from_slice(args);
    let source = template.source();
    let all: Vec<&str> = all
        .into_iter()
        .map(|arg| if arg == "__TEMPLATE__" { &*source } else { arg })
        .collect();
    tpl_outside(
        template.repo.path.parent().expect("parent"),
        template.repo.config_home(),
        &all,
    )
}

#[test]
fn before_seeds_the_sandbox_and_the_render_merges_onto_it() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\n\n\
             [commands]\n\
             before = [\"mkdir -p existing\", \"touch existing/seed.txt\"]\n\
             rendered = [\"ls existing/seed.txt\", \"ls marker.txt\"]\n\
             after = [\"ls existing/seed.txt\", \"ls marker.txt\"]\n\
             finally = [\"ls existing/seed.txt\"]\n",
        )],
    );

    let output = run(&template, &[]).success();
    let json = output.json();
    assert_eq!(json["cases"][0]["passed"], true, "{json}");
    // 2 before + 2 rendered + 2 after + 1 finally.
    assert_eq!(json["cases"][0]["commandsRun"], 7);
    assert_eq!(json["summary"]["commandsRun"], 7);
    assert_eq!(json["summary"]["commandsEnabled"], true);
}

#[test]
fn a_failing_before_command_skips_rendered_and_after_but_still_runs_finally() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\n\n\
             [commands]\n\
             before = [\"ls does-not-exist\"]\n\
             rendered = [\"ls marker.txt\"]\n\
             after = [\"ls marker.txt\"]\n\
             finally = [\"touch finally-ran.txt\"]\n",
        )],
    );

    let output = run(&template, &[]).code(1);
    let json = output.json();
    let failures = &json["cases"][0]["failures"];
    assert_eq!(failures.as_array().unwrap().len(), 1, "{json}");
    assert_eq!(failures[0]["kind"], "commandFailed");
    assert_eq!(failures[0]["step"], "before");
    // 1 before (failed) + 0 rendered (skipped) + 0 after (skipped) + 1 finally.
    assert_eq!(json["cases"][0]["commandsRun"], 2, "{json}");
}

#[test]
fn a_failing_rendered_command_stops_its_own_list_but_after_and_finally_still_run() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\n\n\
             [commands]\n\
             rendered = [\"ls does-not-exist\", \"ls marker.txt\"]\n\
             after = [\"ls marker.txt\"]\n\
             finally = [\"ls marker.txt\"]\n",
        )],
    );

    let output = run(&template, &[]).code(1);
    let json = output.json();
    let failures = &json["cases"][0]["failures"];
    assert_eq!(failures.as_array().unwrap().len(), 1, "{json}");
    assert_eq!(failures[0]["step"], "rendered");
    // 1 rendered (failed, stopped before its second entry) + 1 after + 1 finally.
    assert_eq!(json["cases"][0]["commandsRun"], 3, "{json}");
}

#[test]
fn finally_runs_every_entry_even_when_an_earlier_one_fails() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\n\n\
             [commands]\n\
             finally = [\"ls does-not-exist\", \"ls marker.txt\"]\n",
        )],
    );

    let output = run(&template, &[]).code(1);
    let json = output.json();
    let failures = &json["cases"][0]["failures"];
    assert_eq!(failures.as_array().unwrap().len(), 1, "{json}");
    assert_eq!(failures[0]["step"], "finally");
    // Both `finally` entries are attempted despite the first one failing.
    assert_eq!(json["cases"][0]["commandsRun"], 2, "{json}");
}

#[test]
fn skip_commands_disables_them_entirely() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\n\n[commands]\nbefore = [\"ls does-not-exist\"]\n",
        )],
    );

    let output = run(&template, &["--skip-commands"]).success();
    let json = output.json();
    assert_eq!(json["cases"][0]["passed"], true, "{json}");
    assert_eq!(json["cases"][0]["commandsRun"], 0);
    assert_eq!(json["summary"]["commandsEnabled"], false);
}

/// `tpl.testCommands` is read from the repository containing the current
/// directory, never from the template under test — so it is set on an
/// unrelated "workspace" repository the test runs from, not on the template.
#[test]
fn tpl_test_commands_false_disables_them_without_a_flag() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\n\n[commands]\nbefore = [\"ls does-not-exist\"]\n",
        )],
    );

    let workspace = Repo::init_in(dir.path(), "workspace");
    workspace.git(&["config", "tpl.testCommands", "false"]);

    let source = template.source();
    let output = tpl_outside(
        &workspace.path,
        workspace.config_home(),
        &["--json", "test", &source],
    )
    .success();
    let json = output.json();
    assert_eq!(json["cases"][0]["passed"], true, "{json}");
    assert_eq!(json["summary"]["commandsEnabled"], false);
}

#[test]
fn the_human_output_reports_how_many_commands_ran() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\n\n[commands]\nbefore = [\"touch seed.txt\"]\n",
        )],
    );

    tpl_outside(
        template.repo.path.parent().expect("parent"),
        template.repo.config_home(),
        &["test", &template.source()],
    )
    .success()
    .says("1 command");
}

#[test]
fn a_failing_command_names_the_step_and_the_command() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\n\n[commands]\nbefore = [\"ls does-not-exist\"]\n",
        )],
    );

    tpl_outside(
        template.repo.path.parent().expect("parent"),
        template.repo.config_home(),
        &["test", &template.source()],
    )
    .code(1)
    .says("[before]")
    .says("ls does-not-exist");
}
