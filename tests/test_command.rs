//! `git tpl test` — the template test runner.
//!
//! Real Git throughout: the premise of the project is that Git's behaviour is
//! the behaviour, and a test against a stub would test the stub.

mod common;

use std::path::Path;

use common::{Repo, Template, tpl_outside};

/// A template with a `[answers]`-driven conditional, plus whatever cases the
/// test needs.
///
/// The manifest is deliberately small: what is under test is the runner, and a
/// template with five questions would only make each case harder to read.
fn template(parent: &Path, cases: &[(&str, &str)]) -> Template {
    let built = Template::minimal(
        parent,
        r#"
name = "testable"

[questions.project_name]
type = "string"
prompt = "Name"
default = "demo"

[questions.with_ci]
type = "boolean"
prompt = "CI?"
default = false
"#,
        &[
            ("pyproject.toml.jinja", "name = \"{{ project_name }}\"\n"),
            (
                "{% if with_ci %}ci.yml{% endif %}.jinja",
                "name: {{ project_name }}\n",
            ),
        ],
    );
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
    let mut all = vec!["test", "__TEMPLATE__"];
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

// --- discovery --------------------------------------------------------------

#[test]
fn a_template_with_no_tests_directory_says_so_rather_than_passing_vacuously() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path(), &[]);

    let output = run(&template, &["--json"]).failure();
    assert_eq!(output.error_code(), "tpl::testing::no_tests");
}

#[test]
fn every_case_file_in_the_tests_directory_is_run() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[
            ("tests/a.toml", "[answers]\nproject_name = \"a\"\n"),
            ("tests/b.toml", "[answers]\nproject_name = \"b\"\n"),
        ],
    );

    let output = run(&template, &["--json"]).success();
    let json = output.json();
    assert_eq!(json["summary"]["total"], 2);
    assert_eq!(json["summary"]["passed"], 2);
    assert_eq!(json["cases"][0]["name"], "a");
    assert_eq!(json["cases"][1]["name"], "b");
}

#[test]
fn cases_are_read_in_toml_json_and_yaml() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[
            ("tests/from_toml.toml", "[answers]\nproject_name = \"t\"\n"),
            (
                "tests/from_json.json",
                r#"{"answers": {"project_name": "j"}, "expect": {"files": ["pyproject.toml"]}}"#,
            ),
            (
                "tests/from_yaml.yaml",
                "answers:\n  project_name: y\nexpect:\n  files: [pyproject.toml]\n",
            ),
        ],
    );

    let output = run(&template, &["--json"]).success();
    assert_eq!(output.json()["summary"]["passed"], 3);
}

#[test]
fn a_positional_filter_runs_only_the_named_case() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[
            ("tests/a.toml", "[answers]\nproject_name = \"a\"\n"),
            ("tests/b.toml", "[answers]\nproject_name = \"b\"\n"),
        ],
    );

    let output = run(&template, &["--json", "b"]).success();
    let json = output.json();
    assert_eq!(json["summary"]["total"], 1);
    assert_eq!(json["cases"][0]["name"], "b");
}

#[test]
fn a_filter_that_matches_nothing_fails_with_a_suggestion() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[("tests/minimal.toml", "[answers]\nproject_name = \"a\"\n")],
    );

    // A deliberate near-miss: the point of the case is that a mistyped name is
    // refused with a pointer rather than exiting zero having run nothing.
    let output = run(&template, &["--json", "minimla"]).failure();
    assert_eq!(output.error_code(), "tpl::testing::no_such_case");
    assert!(
        output.json()["error"]["help"]
            .as_str()
            .unwrap()
            .contains("minimal"),
        "expected a suggestion in {}",
        output.stdout
    );
}

#[test]
fn files_that_are_not_cases_are_ignored_rather_than_parsed() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[
            ("tests/minimal.toml", "[answers]\nproject_name = \"a\"\n"),
            // Would be a catastrophic case file, and is not one.
            ("tests/README.md", "# How to write a case\n\nnot [toml\n"),
        ],
    );

    let output = run(&template, &["--json"]).success();
    assert_eq!(output.json()["summary"]["total"], 1);
}

#[test]
fn two_cases_with_the_same_name_in_different_formats_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[
            ("tests/same.toml", "[answers]\nproject_name = \"a\"\n"),
            ("tests/same.yaml", "answers:\n  project_name: b\n"),
        ],
    );

    let output = run(&template, &["--json"]).failure();
    assert_eq!(output.error_code(), "tpl::testing::case_shape");
}

#[test]
fn the_snapshots_directory_is_not_mistaken_for_a_case() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[
            ("tests/minimal.toml", "[answers]\nproject_name = \"a\"\n"),
            // A snapshot manifest is not a case, and neither is a snapshotted
            // TOML file — which this deliberately is.
            (
                "tests/__snapshots__/minimal/files/pyproject.toml",
                "name = \"a\"\n",
            ),
        ],
    );

    let output = run(&template, &["--json"]).failure();
    // The case ran; what failed is that the snapshot has no MANIFEST beside it.
    assert_eq!(output.error_code(), "tpl::testing::snapshot_read");
}

#[test]
fn a_tests_flag_reads_cases_from_another_directory() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[("cases/only.toml", "[answers]\nproject_name = \"a\"\n")],
    );

    run(&template, &["--json"]).failure();
    let output = run(&template, &["--json", "--tests", "cases"]).success();
    assert_eq!(output.json()["cases"][0]["path"], "cases/only.toml");
}

// --- reading from the revision ----------------------------------------------

#[test]
fn cases_are_read_from_the_resolved_revision_not_the_working_tree() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/minimal.toml",
            "[answers]\nproject_name = \"a\"\n\n[expect]\nfiles = [\"pyproject.toml\"]\n",
        )],
    );

    // Broken on disk, but not committed. `--ref`-less means HEAD, and HEAD is
    // still correct.
    template.repo.write(
        "tests/minimal.toml",
        "[answers]\nproject_name = \"a\"\n\n[expect]\nfiles = [\"nope\"]\n",
    );

    run(&template, &["--json"]).success();
}

#[test]
fn dirty_reads_the_uncommitted_cases() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/minimal.toml",
            "[answers]\nproject_name = \"a\"\n\n[expect]\nfiles = [\"pyproject.toml\"]\n",
        )],
    );

    template.repo.write(
        "tests/minimal.toml",
        "[answers]\nproject_name = \"a\"\n\n[expect]\nfiles = [\"nope\"]\n",
    );

    let output = run(&template, &["--json", "--dirty"]).code(1);
    assert_eq!(
        output.json()["cases"][0]["failures"][0]["kind"],
        "missingFile"
    );
}

#[test]
fn a_ref_flag_runs_the_cases_recorded_at_that_tag() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/minimal.toml",
            "[answers]\nproject_name = \"a\"\n\n[expect]\nfiles = [\"pyproject.toml\"]\n",
        )],
    );
    template.repo.git(&["tag", "v1"]);

    // A later commit whose case asserts something false.
    template.repo.write(
        "tests/minimal.toml",
        "[answers]\nproject_name = \"a\"\n\n[expect]\nfiles = [\"nope\"]\n",
    );
    template.repo.commit_all("test: break it");

    run(&template, &["--json", "--ref", "v1"]).success();
    run(&template, &["--json"]).code(1);
}

// --- assertions -------------------------------------------------------------

#[test]
fn a_case_that_only_names_answers_passes_when_the_template_renders() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[("tests/bare.toml", "[answers]\nproject_name = \"a\"\n")],
    );

    let output = run(&template, &["--json"]).success();
    assert!(output.json()["cases"][0]["files"].as_u64().unwrap() > 0);
}

#[test]
fn an_expected_file_that_is_absent_fails_the_case() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\nwith_ci = false\n\n[expect]\nfiles = [\"ci.yml\"]\n",
        )],
    );

    let output = run(&template, &["--json"]).code(1);
    assert_eq!(
        output.json()["cases"][0]["failures"][0]["kind"],
        "missingFile"
    );
    assert_eq!(output.json()["cases"][0]["failures"][0]["path"], "ci.yml");
}

#[test]
fn a_file_expected_to_be_absent_that_is_present_fails_the_case() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\nwith_ci = true\n\n[expect]\nabsent = [\"ci.yml\"]\n",
        )],
    );

    let output = run(&template, &["--json"]).code(1);
    assert_eq!(
        output.json()["cases"][0]["failures"][0]["kind"],
        "unexpectedFile"
    );
}

#[test]
fn a_conditional_path_that_works_is_seen_to_work() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[
            (
                "tests/on.toml",
                "[answers]\nwith_ci = true\n\n[expect]\nfiles = [\"ci.yml\"]\n",
            ),
            (
                "tests/off.toml",
                "[answers]\nwith_ci = false\n\n[expect]\nabsent = [\"ci.yml\"]\n",
            ),
        ],
    );

    run(&template, &["--json"]).success();
}

#[test]
fn a_missing_substring_names_the_file_and_the_text() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\nproject_name = \"thing\"\n\n[expect.contains]\n\"pyproject.toml\" = \"name = \\\"other\\\"\"\n",
        )],
    );

    let output = run(&template, &["--json"]).code(1);
    let failure = &output.json()["cases"][0]["failures"][0];
    assert_eq!(failure["kind"], "containsMissing");
    assert_eq!(failure["path"], "pyproject.toml");
    assert_eq!(failure["needle"], "name = \"other\"");
}

#[test]
fn contains_accepts_a_bare_string_as_well_as_an_array() {
    let dir = tempfile::tempdir().unwrap();
    let built = template(
        dir.path(),
        &[
            (
                "tests/bare.toml",
                "[answers]\nproject_name = \"thing\"\n\n[expect.contains]\n\"pyproject.toml\" = \"thing\"\n",
            ),
            (
                "tests/array.toml",
                "[answers]\nproject_name = \"thing\"\n\n[expect.contains]\n\"pyproject.toml\" = [\"thing\", \"name\"]\n",
            ),
        ],
    );

    let output = run(&built, &["--json"]).success();
    assert_eq!(output.json()["summary"]["passed"], 2);
}

#[test]
fn a_case_expecting_an_error_passes_when_the_render_fails_with_that_code() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\nwith_ci = \"not a bool\"\n\n[expect]\nerror = \"tpl::eval::wrong_type\"\n",
        )],
    );

    run(&template, &["--json"]).success();
}

#[test]
fn an_expected_code_matches_a_cause_rather_than_only_the_outermost_error() {
    let dir = tempfile::tempdir().unwrap();
    let built = Template::minimal(
        dir.path(),
        "name = \"strict\"\nstrict = true\n",
        &[("a.txt.jinja", "{{ undeclared_name }}\n")],
    );
    // The outer failure is `tpl::render::content`; only the cause names the
    // expression. A case must be able to name either.
    built.repo.write(
        "tests/c.toml",
        "[expect]\nerror = \"tpl::eval::expression\"\n",
    );
    built.repo.commit_all("test: case");

    run(&built, &["--json"]).success();

    built.repo.write(
        "tests/c.toml",
        "[expect]\nerror = \"tpl::render::content\"\n",
    );
    built.repo.commit_all("test: outer code");
    run(&built, &["--json"]).success();
}

#[test]
fn a_render_that_fails_with_a_different_code_reports_both() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\nwith_ci = \"not a bool\"\n\n[expect]\nerror = \"tpl::render::collision\"\n",
        )],
    );

    let output = run(&template, &["--json"]).code(1);
    let failure = &output.json()["cases"][0]["failures"][0];
    assert_eq!(failure["kind"], "wrongError");
    assert_eq!(failure["expected"], "tpl::render::collision");
    assert!(
        failure["actual"]
            .as_array()
            .unwrap()
            .iter()
            .any(|code| code == "tpl::eval::wrong_type"),
        "expected the real codes in {failure}"
    );
}

#[test]
fn a_case_expecting_an_error_fails_when_the_render_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\nproject_name = \"fine\"\n\n[expect]\nerror = \"tpl::eval::wrong_type\"\n",
        )],
    );

    let output = run(&template, &["--json"]).code(1);
    assert_eq!(
        output.json()["cases"][0]["failures"][0]["kind"],
        "expectedError"
    );
}

#[test]
fn an_unexpected_render_failure_fails_only_that_case() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[
            ("tests/fine.toml", "[answers]\nproject_name = \"a\"\n"),
            ("tests/broken.toml", "[answers]\nwith_ci = \"not a bool\"\n"),
        ],
    );

    let output = run(&template, &["--json"]).code(1);
    let json = output.json();
    assert_eq!(json["summary"]["passed"], 1);
    assert_eq!(json["summary"]["failed"], 1);
    assert_eq!(json["cases"][0]["name"], "broken");
    assert_eq!(
        json["cases"][0]["failures"][0]["kind"], "unexpectedError",
        "a failing render is the case's failure, not the run's"
    );
    assert_eq!(json["cases"][1]["passed"], true);
}

#[test]
fn an_unknown_key_in_a_case_file_is_refused_rather_than_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[("tests/c.toml", "[expects]\nfiles = [\"pyproject.toml\"]\n")],
    );

    let output = run(&template, &["--json"]).failure();
    assert_eq!(output.error_code(), "tpl::testing::case_shape");
    assert!(
        output.json()["error"]["help"]
            .as_str()
            .unwrap()
            .contains("expect"),
        "expected a suggestion"
    );
}

#[test]
fn a_case_cannot_expect_an_error_and_a_file_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[expect]\nerror = \"tpl::eval::wrong_type\"\nfiles = [\"a\"]\n",
        )],
    );

    let output = run(&template, &["--json"]).failure();
    assert_eq!(output.error_code(), "tpl::testing::case_shape");
}

#[test]
fn a_case_file_that_does_not_parse_names_the_case() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path(), &[("tests/c.toml", "[answers\nbroken\n")]);

    let output = run(&template, &["--json"]).failure();
    assert_eq!(output.error_code(), "tpl::testing::case_parse");
    assert!(output.stdout.contains("tests/c.toml"));
}

// --- snapshots --------------------------------------------------------------

#[test]
fn write_records_the_rendered_tree_under_the_snapshots_directory() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/minimal.toml",
            "[answers]\nproject_name = \"thing\"\n",
        )],
    );

    let output = run(&template, &["--json", "--write"]).success();
    assert_eq!(output.json()["cases"][0]["snapshot"], "written");

    assert_eq!(
        template
            .repo
            .read("tests/__snapshots__/minimal/files/pyproject.toml"),
        "name = \"thing\"\n",
        "the rendered bytes, verbatim, at the rendered path"
    );
    let manifest = template.repo.read("tests/__snapshots__/minimal/MANIFEST");
    assert!(manifest.starts_with("# git-tpl snapshot 1\n"), "{manifest}");
    assert!(manifest.contains(" pyproject.toml\n"), "{manifest}");
}

#[test]
fn a_recorded_snapshot_is_compared_on_the_next_run() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/minimal.toml",
            "[answers]\nproject_name = \"thing\"\n",
        )],
    );

    run(&template, &["--write"]).success();
    template.repo.commit_all("test: record the snapshot");

    let output = run(&template, &["--json"]).success();
    assert_eq!(output.json()["cases"][0]["snapshot"], "compared");
    assert_eq!(output.json()["summary"]["snapshotsCompared"], 1);
}

#[test]
fn a_changed_template_makes_the_snapshot_diff_name_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/minimal.toml",
            "[answers]\nproject_name = \"thing\"\n",
        )],
    );

    run(&template, &["--write"]).success();
    template.repo.commit_all("test: record the snapshot");

    template.repo.write(
        "template/pyproject.toml.jinja",
        "name = \"{{ project_name }}\"\nnew = 1\n",
    );
    template.repo.commit_all("feat: add a line");

    let output = run(&template, &["--json"]).code(1);
    let failure = &output.json()["cases"][0]["failures"][0];
    assert_eq!(failure["kind"], "snapshotDiff");
    assert_eq!(failure["changes"][0]["path"], "pyproject.toml");
    assert_eq!(failure["changes"][0]["kind"], "modified");
    assert!(
        failure["changes"][0]["patch"]
            .as_str()
            .unwrap()
            .contains("+new = 1"),
        "expected a unified diff in {failure}"
    );
}

#[test]
fn write_over_an_existing_snapshot_removes_files_the_template_no_longer_produces() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/minimal.toml",
            "[answers]\nproject_name = \"thing\"\nwith_ci = true\n",
        )],
    );

    run(&template, &["--write"]).success();
    assert!(
        template
            .repo
            .path
            .join("tests/__snapshots__/minimal/files/ci.yml")
            .exists()
    );

    // The template stops producing it. A snapshot that kept the stale file
    // would let the author conclude their conditional works.
    template.repo.write(
        "tests/minimal.toml",
        "[answers]\nproject_name = \"thing\"\nwith_ci = false\n",
    );
    template.repo.commit_all("test: turn CI off");

    run(&template, &["--write"]).success();
    assert!(
        !template
            .repo
            .path
            .join("tests/__snapshots__/minimal/files/ci.yml")
            .exists(),
        "the snapshot directory is cleared, not merged into"
    );
}

#[test]
fn write_reports_a_snapshot_that_did_not_change_as_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/minimal.toml",
            "[answers]\nproject_name = \"thing\"\n",
        )],
    );

    run(&template, &["--write"]).success();
    template.repo.commit_all("test: record the snapshot");

    let output = run(&template, &["--json", "--write"]).success();
    assert_eq!(
        output.json()["cases"][0]["snapshot"],
        "unchanged",
        "a green suite must not claim to have rewritten anything"
    );
    assert_eq!(output.json()["summary"]["snapshotsWritten"], 0);
}

#[test]
fn a_case_with_no_snapshot_is_not_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[
            ("tests/recorded.toml", "[answers]\nproject_name = \"a\"\n"),
            ("tests/bare.toml", "[answers]\nproject_name = \"b\"\n"),
        ],
    );

    run(&template, &["--write", "recorded"]).success();
    template.repo.commit_all("test: one snapshot only");

    let output = run(&template, &["--json"]).success();
    let json = output.json();
    assert_eq!(json["cases"][0]["snapshot"], "none", "bare");
    assert_eq!(json["cases"][1]["snapshot"], "compared", "recorded");
}

#[cfg(unix)]
#[test]
fn the_snapshot_manifest_records_the_executable_bit() {
    let dir = tempfile::tempdir().unwrap();
    let built = Template::minimal(
        dir.path(),
        "name = \"exe\"\n",
        &[("run.sh", "#!/bin/sh\necho hi\n")],
    );
    built.repo.make_executable("template/run.sh");
    built.repo.write("tests/c.toml", "[answers]\n");
    built.repo.commit_all("test: an executable");

    run(&built, &["--write"]).success();
    let manifest = built.repo.read("tests/__snapshots__/c/MANIFEST");
    assert!(
        manifest.contains("100755 "),
        "the executable mode is recorded: {manifest}"
    );

    built.repo.commit_all("test: record the snapshot");

    // Drop the bit in the template. Only the mode changed, and the case must
    // still fail — on Windows the file on disk cannot carry it, which is why
    // the manifest does.
    built
        .repo
        .git(&["update-index", "--chmod=-x", "template/run.sh"]);
    built
        .repo
        .git(&["commit", "-q", "-m", "chore: drop the bit"]);

    let output = run(&built, &["--json"]).code(1);
    let change = &output.json()["cases"][0]["failures"][0]["changes"][0];
    assert_eq!(change["path"], "run.sh");
    assert_eq!(change["modeOnly"], true);
}

#[test]
fn a_binary_file_round_trips_through_a_snapshot_without_a_patch() {
    let dir = tempfile::tempdir().unwrap();
    let built = Template::minimal(dir.path(), "name = \"bin\"\n", &[]);
    std::fs::create_dir_all(built.repo.path.join("template")).unwrap();
    std::fs::write(built.repo.path.join("template/logo.png"), [0u8, 1, 2, 3]).unwrap();
    built.repo.write("tests/c.toml", "[answers]\n");
    built.repo.commit_all("test: a binary file");

    run(&built, &["--write"]).success();
    let manifest = built.repo.read("tests/__snapshots__/c/MANIFEST");
    assert!(
        manifest.contains(" binary "),
        "a binary file records no digest: {manifest}"
    );
    built.repo.commit_all("test: record");

    std::fs::write(built.repo.path.join("template/logo.png"), [0u8, 9, 9, 9]).unwrap();
    built.repo.commit_all("chore: change the bytes");

    let output = run(&built, &["--json"]).code(1);
    let change = &output.json()["cases"][0]["failures"][0]["changes"][0];
    assert_eq!(change["kind"], "modified");
    assert!(change["patch"].is_null(), "no patch for binary");
}

#[test]
fn write_is_refused_on_a_template_with_no_working_tree() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[("tests/c.toml", "[answers]\nproject_name = \"a\"\n")],
    );

    let bare = dir.path().join("bare.git");
    Repo::at(template.repo.path.clone()).git(&[
        "clone",
        "--bare",
        "-q",
        &template.source(),
        bare.to_str().unwrap(),
    ]);

    let output = tpl_outside(
        dir.path(),
        template.repo.config_home(),
        &[
            "--json",
            "test",
            &format!("file://{}", bare.display()),
            "--write",
        ],
    )
    .failure();
    assert_eq!(output.error_code(), "tpl::testing::write_needs_local");
}

#[test]
fn write_does_not_stage_or_commit_the_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[("tests/c.toml", "[answers]\nproject_name = \"a\"\n")],
    );
    let before = template.repo.git(&["rev-parse", "HEAD"]);

    run(&template, &["--write"]).success();

    assert_eq!(
        template.repo.git(&["rev-parse", "HEAD"]),
        before,
        "recording a snapshot must not commit"
    );
    let status = template.repo.git(&["status", "--porcelain"]);
    assert!(
        status.contains("?? tests/__snapshots__/"),
        "the snapshot is untracked, for the author to review and commit: {status}"
    );
}

#[test]
fn write_still_fails_a_case_whose_expectations_are_unmet() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\nproject_name = \"a\"\n\n[expect]\nfiles = [\"nope\"]\n",
        )],
    );

    // Recording a rendering is not blessing it.
    let output = run(&template, &["--json", "--write"]).code(1);
    assert_eq!(output.json()["cases"][0]["snapshot"], "written");
    assert_eq!(
        output.json()["cases"][0]["failures"][0]["kind"],
        "missingFile"
    );
}

// --- behaviour and plumbing -------------------------------------------------

#[test]
fn a_failing_case_exits_one_and_a_passing_suite_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[("tests/c.toml", "[answers]\nproject_name = \"a\"\n")],
    );
    run(&template, &[]).code(0);

    template.repo.write(
        "tests/c.toml",
        "[answers]\nproject_name = \"a\"\n\n[expect]\nfiles = [\"nope\"]\n",
    );
    template.repo.commit_all("test: break it");
    run(&template, &[]).code(1);
}

#[test]
fn the_json_report_is_ok_even_when_a_case_fails() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\nproject_name = \"a\"\n\n[expect]\nfiles = [\"nope\"]\n",
        )],
    );

    let output = run(&template, &["--json"]).code(1);
    let json = output.json();
    // `ok` says the command ran; `summary.failed` says what it found. A caller
    // has to be able to tell a failing case from an unresolvable template.
    assert_eq!(json["ok"], true);
    assert_eq!(json["summary"]["failed"], 1);
    assert!(json["error"].is_null());
}

#[test]
fn the_json_report_names_the_revision_every_case_ran_against() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[
            ("tests/a.toml", "[answers]\nproject_name = \"a\"\n"),
            ("tests/b.toml", "[answers]\nproject_name = \"b\"\n"),
            ("tests/c.toml", "[answers]\nproject_name = \"c\"\n"),
        ],
    );

    let output = run(&template, &["--json"]).success();
    let json = output.json();
    let head = template.repo.git(&["rev-parse", "HEAD"]);
    assert_eq!(json["revision"]["commit"], head.trim());
    assert_eq!(json["summary"]["total"], 3, "one revision, three cases");
}

#[test]
fn a_test_run_does_not_touch_the_template_working_tree() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[("tests/c.toml", "[answers]\nproject_name = \"a\"\n")],
    );
    let before = template.repo.working_state();

    run(&template, &[]).success();

    assert_eq!(
        template.repo.working_state(),
        before,
        "without --write, `test` writes nothing anywhere"
    );
}

#[test]
fn a_case_that_leaves_a_question_unanswered_reports_the_unanswered_question() {
    let dir = tempfile::tempdir().unwrap();
    let built = Template::minimal(
        dir.path(),
        r#"
name = "needs-an-answer"

[questions.required]
type = "string"
prompt = "Required"
"#,
        &[("a.txt.jinja", "{{ required }}\n")],
    );
    built.repo.write("tests/c.toml", "[answers]\n");
    built.repo.commit_all("test: a case with no answer");

    // A failure, not a hang: nothing is ever prompted for.
    let output = run(&built, &["--json"]).code(1);
    assert_eq!(
        output.json()["cases"][0]["failures"][0]["kind"],
        "unexpectedError"
    );
}

#[test]
fn the_human_output_names_each_case_and_the_totals() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[
            ("tests/passing.toml", "[answers]\nproject_name = \"a\"\n"),
            (
                "tests/failing.toml",
                "[answers]\nproject_name = \"a\"\n\n[expect]\nfiles = [\"nope\"]\n",
            ),
        ],
    );

    run(&template, &[])
        .code(1)
        .says("passing")
        .says("FAILED")
        .says("missing file")
        .says("1 passed, 1 failed");
}
