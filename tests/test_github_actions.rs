//! `git tpl test`'s GitHub Actions progress reporter — ADR-035.
//!
//! Real Git and a real spawned binary throughout, `GITHUB_ACTIONS=true`
//! injected into the child's environment via `tpl_with_env`.

mod common;

use std::path::Path;

use common::{Template, tpl_with_env};

fn template(parent: &Path, cases: &[(&str, &str)]) -> Template {
    let built = Template::minimal(parent, "name = \"ghatest\"\n", &[("marker.txt", "hello\n")]);
    for (path, body) in cases {
        built.repo.write(path, body);
    }
    if !cases.is_empty() {
        built.repo.commit_all("test: cases");
    }
    built
}

/// Run `git tpl test` under a simulated GitHub Actions job.
fn run_gha(template: &Template, args: &[&str]) -> common::Output {
    let mut all = vec!["test"];
    all.extend_from_slice(args);
    tpl_with_env(&template.repo, &all, &[("GITHUB_ACTIONS", "true")])
}

#[test]
fn github_actions_mode_wraps_each_case_in_a_group() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[
            ("tests/a.toml", "[answers]\n"),
            ("tests/b.toml", "[answers]\n"),
        ],
    );

    let output = run_gha(&template, &[]).success();
    let stdout = output.stdout.clone();

    assert!(stdout.contains("::group::a"), "{stdout}");
    assert!(stdout.contains("::group::b"), "{stdout}");
    assert_eq!(stdout.matches("::endgroup::").count(), 2, "{stdout}");

    // `a`'s group opens and closes entirely before `b`'s opens — cases run
    // serially, and the fold must not straddle two of them.
    let a_group = stdout.find("::group::a").unwrap();
    let a_end = stdout[a_group..].find("::endgroup::").unwrap() + a_group;
    let b_group = stdout.find("::group::b").unwrap();
    assert!(a_end < b_group, "{stdout}");
}

#[test]
fn a_failing_case_gets_exactly_one_error_annotation_naming_its_own_path() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/bad.toml",
            "[answers]\n\n\
             [expect]\n\
             absent = [\"marker.txt\"]\n",
        )],
    );

    let output = run_gha(&template, &[]).code(1);
    let stdout = &output.stdout;

    assert_eq!(stdout.matches("::error ").count(), 1, "{stdout}");
    assert!(stdout.contains("::error file=tests/bad.toml"), "{stdout}");
    assert!(stdout.contains("case `bad` failed"), "{stdout}");
}

#[test]
fn a_passing_suite_emits_no_error_annotations() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path(), &[("tests/a.toml", "[answers]\n")]);

    let output = run_gha(&template, &[]).success();
    assert!(!output.stdout.contains("::error "), "{}", output.stdout);
}

#[test]
fn phase_chatter_is_emitted_as_debug_not_plain_output() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path(), &[("tests/a.toml", "[answers]\n")]);

    let output = run_gha(&template, &[]).success();
    assert!(
        output.stdout.contains("::debug::a: [rendering]"),
        "{}",
        output.stdout
    );
    // Not repeated as a plain, unprefixed line: the whole point of `::debug::`
    // is that it only surfaces under step debug logging.
    assert_eq!(
        output.stdout.matches("[rendering]").count(),
        1,
        "{}",
        output.stdout
    );
}

/// Real `coreutils`, on the same principle `tests/test_commands.rs` already
/// applies — gated to `unix` because that is where `echo` is guaranteed.
#[cfg(unix)]
#[test]
fn command_output_is_forwarded_live_inside_the_group_regardless_of_verbose() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/a.toml",
            "[answers]\n\n\
             [commands]\n\
             rendered = [\"echo distinctive-marker-xyz\"]\n",
        )],
    );

    // Deliberately no `-v`: GitHub Actions mode forwards unconditionally.
    let output = run_gha(&template, &[]).success();
    assert!(
        output.stdout.contains("distinctive-marker-xyz"),
        "{}",
        output.stdout
    );

    let group_start = output.stdout.find("::group::a").unwrap();
    let group_end = output.stdout[group_start..].find("::endgroup::").unwrap() + group_start;
    let marker = output.stdout.find("distinctive-marker-xyz").unwrap();
    assert!(
        marker > group_start && marker < group_end,
        "{}",
        output.stdout
    );
}

#[test]
fn github_actions_mode_is_not_activated_under_json() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path(), &[("tests/a.toml", "[answers]\n")]);

    let output = run_gha(&template, &["--json"]).success();
    // `Output::json` already panics, with the full transcript, if stdout is
    // not clean single-object JSON — which is exactly what a stray
    // `::group::`/`::debug::` line would break.
    let json = output.json();
    assert_eq!(json["summary"]["total"], 1);
    assert!(!output.stdout.contains("::group::"), "{}", output.stdout);
}

#[test]
fn github_actions_mode_is_not_activated_under_quiet() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path(), &[("tests/a.toml", "[answers]\n")]);

    let output = run_gha(&template, &["--quiet"]).success();
    assert_eq!(output.stdout, "", "{}", output.stdout);
    assert!(!output.all().contains("::group::"), "{}", output.all());
}
