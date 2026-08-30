//! `git tpl test` — the template test runner.
//!
//! Real Git throughout: the premise of the project is that Git's behaviour is
//! the behaviour, and a test against a stub would test the stub.

mod common;

use std::path::Path;

use common::{Repo, Template, tpl, tpl_outside};

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

/// Run `git tpl test` from inside the template's own repository. `--template`
/// defaults to `.`, so none of these cases need to pass it explicitly.
fn run(template: &Template, args: &[&str]) -> common::Output {
    let mut all = vec!["test"];
    all.extend_from_slice(args);
    tpl(&template.repo, &all)
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

/// Also the regression case for keeping `--template` a flag rather than a
/// positional: a bare case name here must never be read as a template path.
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
            (
                "tests/minimal.toml",
                "snapshot = true\n[answers]\nproject_name = \"a\"\n",
            ),
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

// --- dirty by default, `--ref` pins a committed revision --------------------

#[test]
fn an_uncommitted_case_change_is_read_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/minimal.toml",
            "[answers]\nproject_name = \"a\"\n\n[expect]\nfiles = [\"pyproject.toml\"]\n",
        )],
    );

    // Broken on disk, but not committed. With no `--ref`, the working tree is
    // read (ADR-030), so the broken case is what runs.
    template.repo.write(
        "tests/minimal.toml",
        "[answers]\nproject_name = \"a\"\n\n[expect]\nfiles = [\"nope\"]\n",
    );

    let output = run(&template, &["--json"]).code(1);
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

// --- `TEMPLATE` names a checkout ---------------------------------------------

/// The point of keeping `--template` at all (ADR-030): a script that has not
/// `cd`ed into the template can still name it, without needing a remote
/// source or a manifest-root override.
#[test]
fn a_template_named_by_a_relative_path_is_testable_without_cding_into_it() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/minimal.toml",
            "[answers]\nproject_name = \"a\"\n\n[expect]\nfiles = [\"pyproject.toml\"]\n",
        )],
    );

    // `Repo::init_in` always names the template's directory "template".
    let output = tpl_outside(
        dir.path(),
        template.repo.config_home(),
        &["--json", "test", "--template", "template"],
    )
    .success();
    assert_eq!(output.json()["summary"]["passed"], 1);
}

/// Unlike every other command, `test` never resolves a remote source — there
/// is no committed-revision story for it the way there is for `render`, and
/// refusing it is unconditional: naming `--ref` does not make a remote
/// `--template` acceptable, because there is still no working tree to read.
#[test]
fn a_remote_source_is_refused_even_with_a_ref() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[("tests/c.toml", "[answers]\nproject_name = \"a\"\n")],
    );
    template.repo.git(&["tag", "v1"]);

    let bare = dir.path().join("bare.git");
    Repo::at(template.repo.path.clone()).git(&[
        "clone",
        "--bare",
        "-q",
        &template.source(),
        bare.to_str().unwrap(),
    ]);
    let url = common::file_url(&bare);

    let output = tpl_outside(
        dir.path(),
        template.repo.config_home(),
        &["--json", "test", "--template", &url],
    )
    .failure();
    assert_eq!(output.error_code(), "tpl::testing::remote_not_supported");

    let output = tpl_outside(
        dir.path(),
        template.repo.config_home(),
        &["--json", "test", "--template", &url, "--ref", "v1"],
    )
    .failure();
    assert_eq!(output.error_code(), "tpl::testing::remote_not_supported");
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
fn a_forbidden_substring_that_is_truly_absent_passes() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\nproject_name = \"thing\"\n\n[expect.lacks]\n\"pyproject.toml\" = \"other\"\n",
        )],
    );

    run(&template, &["--json"]).success();
}

#[test]
fn a_forbidden_substring_present_names_the_file_and_the_text() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\nproject_name = \"thing\"\n\n[expect.lacks]\n\"pyproject.toml\" = \"thing\"\n",
        )],
    );

    let output = run(&template, &["--json"]).code(1);
    let failure = &output.json()["cases"][0]["failures"][0];
    assert_eq!(failure["kind"], "lacksPresent");
    assert_eq!(failure["path"], "pyproject.toml");
    assert_eq!(failure["needle"], "thing");
}

#[test]
fn lacks_accepts_a_bare_string_as_well_as_an_array() {
    let dir = tempfile::tempdir().unwrap();
    let built = template(
        dir.path(),
        &[
            (
                "tests/bare.toml",
                "[answers]\nproject_name = \"thing\"\n\n[expect.lacks]\n\"pyproject.toml\" = \"other\"\n",
            ),
            (
                "tests/array.toml",
                "[answers]\nproject_name = \"thing\"\n\n[expect.lacks]\n\"pyproject.toml\" = [\"other\", \"else\"]\n",
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
            "snapshot = true\n[answers]\nproject_name = \"thing\"\n",
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
            "snapshot = true\n[answers]\nproject_name = \"thing\"\n",
        )],
    );

    run(&template, &["--write"]).success();
    template.repo.commit_all("test: record the snapshot");

    let output = run(&template, &["--json"]).success();
    assert_eq!(output.json()["cases"][0]["snapshot"], "compared");
    assert_eq!(output.json()["summary"]["snapshotsCompared"], 1);
}

/// #51. `--write` records a snapshot through the filesystem; the dirty
/// read-back reads it through the working-tree walk. The two halves have to
/// agree, and they did not: a global `core.excludesFile` hiding `mise.toml`
/// made the walk drop `files/mise.toml` while leaving the `MANIFEST` that
/// lists it, so recording a snapshot made every subsequent run fail with
/// `snapshot_read`.
///
/// The negation is not contrived. A template that renders a `mise.toml` ships
/// `!mise.toml` in its `.gitignore`, and the *rendered* `.gitignore` then
/// governs the snapshot's own `files/` directory. Note the asymmetry that hid
/// it: in the template the file is `mise.toml.jinja`, which the rule does not
/// match, so only the snapshot is affected.
#[test]
fn a_snapshot_of_a_globally_ignored_filename_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let template = Template::minimal(
        dir.path(),
        "name = \"mised\"\n",
        &[
            ("mise.toml.jinja", "[tools]\nrust = \"stable\"\n"),
            (".gitignore", "!mise.toml\n"),
        ],
    );
    template
        .repo
        .write("tests/case.toml", "snapshot = true\n[answers]\n");
    template.repo.commit_all("test: a case");

    // The rule the whole bug depends on, in an isolated config so the test
    // never touches the developer's own ignore rules.
    common::global_gitignore(template.repo.config_home(), "mise.toml\nmise.lock\n");

    run(&template, &["--write"]).success();
    assert!(
        template
            .repo
            .path
            .join("tests/__snapshots__/case/files/mise.toml")
            .exists(),
        "`--write` did not record the file at all"
    );

    // The read-back. Before the fix this was `tpl::testing::snapshot_read`:
    // "`MANIFEST` lists `mise.toml`, which is not under `files/`".
    let output = run(&template, &["--json"]).success();
    assert_eq!(output.json()["cases"][0]["snapshot"], "compared");
}

/// #116. Distinct from #51 above: no negation involved, just an ordinary
/// rule — a bare `MANIFEST`, as Python's `setup.py sdist` convention writes —
/// matching the snapshot's own manifest file at any depth. `--write` put the
/// file on disk regardless, since it never goes through Git at all; before
/// the fix, the dirty read-back dropped it from the synthetic tree anyway and
/// reported `tpl::testing::snapshot_read`: "there is no `MANIFEST`".
#[test]
fn a_snapshot_whose_manifest_matches_an_ordinary_gitignore_rule_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[("tests/case.toml", "snapshot = true\n[answers]\n")],
    );
    template.repo.write(".gitignore", "MANIFEST\n");
    template.repo.commit_all("test: an ordinary MANIFEST rule");

    run(&template, &["--write"]).success();
    assert!(
        template
            .repo
            .git(&["ls-files", "tests/__snapshots__/case/MANIFEST"])
            .is_empty(),
        "sanity: the rule really does keep git from tracking it"
    );

    let output = run(&template, &["--json"]).success();
    assert_eq!(output.json()["cases"][0]["snapshot"], "compared");
}

/// #83, as reported: through `git tpl test`, where the warning was the first
/// thing printed and pushed the actual results one line down. `.opencode/` is
/// beside `template.toml` rather than under `root`, and the rule hiding it is
/// global — so there was nothing the template could do about it.
#[test]
fn an_ignored_path_outside_the_render_root_is_not_warned_about() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path(), &[("tests/case.toml", "[answers]\n")]);

    // Left uncommitted, as a tool-created directory beside a template is.
    template.repo.write(".opencode/plans/one.md", "a plan\n");
    common::global_gitignore(template.repo.config_home(), ".opencode/\n");

    run(&template, &[])
        .success()
        .silent_about("skipped by .gitignore");
}

#[test]
fn a_changed_template_makes_the_snapshot_diff_name_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/minimal.toml",
            "snapshot = true\n[answers]\nproject_name = \"thing\"\n",
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
            "snapshot = true\n[answers]\nproject_name = \"thing\"\nwith_ci = true\n",
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
        "snapshot = true\n[answers]\nproject_name = \"thing\"\nwith_ci = false\n",
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
            "snapshot = true\n[answers]\nproject_name = \"thing\"\n",
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
            (
                "tests/recorded.toml",
                "snapshot = true\n[answers]\nproject_name = \"a\"\n",
            ),
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

/// Explicit opt-in must be enforced, not silently skipped: a case that says
/// `snapshot = true` but was never recorded (no prior `--write`) fails
/// outright rather than reporting `none`.
#[test]
fn snapshot_true_with_nothing_recorded_yet_is_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/minimal.toml",
            "snapshot = true\n[answers]\nproject_name = \"thing\"\n",
        )],
    );

    let output = run(&template, &["--json"]).code(1);
    let json = output.json();
    assert_eq!(json["cases"][0]["snapshot"], "none");
    assert_eq!(json["cases"][0]["failures"][0]["kind"], "snapshotMissing");
}

/// `--write` only records a snapshot for a case that asked for one — a case
/// with `snapshot` unset must not get a `__snapshots__` directory at all,
/// even under `--write`.
#[test]
fn write_does_not_record_a_snapshot_for_a_case_that_did_not_ask_for_one() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[("tests/bare.toml", "[answers]\nproject_name = \"a\"\n")],
    );

    let output = run(&template, &["--json", "--write"]).success();
    assert_eq!(output.json()["cases"][0]["snapshot"], "none");
    assert!(
        !template.repo.path.join("tests/__snapshots__/bare").exists(),
        "a case that never asked for a snapshot must not get one written"
    );
}

/// `snapshot = true` combined with `expect.error` is a contradiction — a
/// render that never succeeds has nothing to snapshot — refused at parse
/// time, the same way `expect.error` with `expect.files` already is.
#[test]
fn snapshot_true_cannot_combine_with_expect_error() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "snapshot = true\n[answers]\nproject_name = \"a\"\n\n\
             [expect]\nerror = \"tpl::eval::unanswered\"\n",
        )],
    );

    let output = run(&template, &["--json"]).failure();
    assert_eq!(output.error_code(), "tpl::testing::case_shape");
}

/// `commands.rendered`/`commands.after` need a rendering to run against, so
/// `expect.error` — which says the render never succeeds — cannot combine
/// with either, the same contradiction `expect.error` already refuses for
/// `files`/`absent`/`contains`/`lacks`.
#[test]
fn commands_rendered_cannot_combine_with_expect_error() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\nproject_name = \"a\"\n\n\
             [expect]\nerror = \"tpl::eval::unanswered\"\n\n\
             [commands]\nrendered = [\"ls\"]\n",
        )],
    );

    let output = run(&template, &["--json"]).failure();
    assert_eq!(output.error_code(), "tpl::testing::case_shape");
}

/// An unknown `[commands]` key is refused with a suggestion, the same
/// strictness the top-level and `[expect]` keys already get.
#[test]
fn an_unknown_commands_key_is_refused_with_a_suggestion() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\n\n[commands]\nbefor = [\"ls\"]\n",
        )],
    );

    let output = run(&template, &["--json"]).failure();
    assert_eq!(output.error_code(), "tpl::testing::case_shape");
    assert!(output.stdout.contains("before"), "{}", output.stdout);
}

/// `commands.env`, and a list written as a table with its own `env`, both
/// parse — the shape-level half of issue #130, run separately from whether
/// the environment actually reaches a spawned process (`test_commands.rs`).
#[cfg(unix)]
#[test]
fn commands_env_and_a_per_list_env_override_both_parse() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\n\n\
             [commands]\n\
             env = { GLOBAL = \"1\" }\n\
             before = [\"true\"]\n\n\
             [commands.rendered]\n\
             env = { LOCAL = \"2\" }\n\
             run = [\"true\"]\n",
        )],
    );

    let output = run(&template, &["--json"]).success();
    let json = output.json();
    assert_eq!(json["cases"][0]["passed"], true, "{json}");
}

/// A non-string value inside `commands.env` is refused, the same strictness
/// `commands.before`'s own strings already get.
#[test]
fn a_non_string_commands_env_value_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\n\n[commands]\nenv = { GLOBAL = 1 }\n",
        )],
    );

    let output = run(&template, &["--json"]).failure();
    assert_eq!(output.error_code(), "tpl::testing::case_shape");
}

/// An unknown key inside a list written as a table (`[commands.rendered]`)
/// is refused with a suggestion, the same strictness `[commands]` itself
/// already gets for `before`/`rendered`/`after`/`finally`/`env`.
#[test]
fn an_unknown_key_inside_a_command_list_table_is_refused_with_a_suggestion() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\n\n[commands.rendered]\nrunn = [\"true\"]\n",
        )],
    );

    let output = run(&template, &["--json"]).failure();
    assert_eq!(output.error_code(), "tpl::testing::case_shape");
    assert!(output.stdout.contains("run"), "{}", output.stdout);
}

/// `commands.before` that is neither an array nor a table is refused,
/// naming both shapes it does accept.
#[test]
fn a_command_list_that_is_neither_an_array_nor_a_table_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[("tests/c.toml", "[answers]\n\n[commands]\nbefore = \"ls\"\n")],
    );

    let output = run(&template, &["--json"]).failure();
    assert_eq!(output.error_code(), "tpl::testing::case_shape");
    assert!(
        output.stdout.contains("`run` and `env`"),
        "{}",
        output.stdout
    );
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
    built
        .repo
        .write("tests/c.toml", "snapshot = true\n[answers]\n");
    built.repo.commit_all("test: an executable");

    run(&built, &["--write"]).success();
    let manifest = built.repo.read("tests/__snapshots__/c/MANIFEST");
    assert!(
        manifest.contains("100755 "),
        "the executable mode is recorded: {manifest}"
    );

    built.repo.commit_all("test: record the snapshot");

    // Drop the bit in the template's committed *mode*, not on disk — on
    // Windows the file on disk cannot carry it, which is why the manifest
    // does, and it is why this asserts against `--ref HEAD` rather than the
    // default working-tree read: the disk copy still has the bit set, and
    // only Git's own record of the mode changed.
    built
        .repo
        .git(&["update-index", "--chmod=-x", "template/run.sh"]);
    built
        .repo
        .git(&["commit", "-q", "-m", "chore: drop the bit"]);

    let output = run(&built, &["--json", "--ref", "HEAD"]).code(1);
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
    built
        .repo
        .write("tests/c.toml", "snapshot = true\n[answers]\n");
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
fn write_does_not_stage_or_commit_the_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "snapshot = true\n[answers]\nproject_name = \"a\"\n",
        )],
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
            "snapshot = true\n[answers]\nproject_name = \"a\"\n\n[expect]\nfiles = [\"nope\"]\n",
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

/// A snapshot that has been hand-edited into disagreeing with itself.
///
/// The MANIFEST is authoritative for the file list and modes, `files/` for
/// content. Trusting either half alone would let a snapshot drift into
/// asserting nothing while still reporting green, which is the worst failure
/// mode a snapshot suite has.
fn corrupted_snapshot(dir: &Path, corrupt: impl Fn(&Template)) -> common::Output {
    let built = template(
        dir,
        &[(
            "tests/minimal.toml",
            "snapshot = true\n[answers]\nproject_name = \"thing\"\n",
        )],
    );
    run(&built, &["--write"]).success();
    corrupt(&built);
    built.repo.commit_all("test: a corrupted snapshot");
    run(&built, &["--json"])
}

#[test]
fn a_snapshot_whose_manifest_lists_a_file_that_is_not_there_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let output = corrupted_snapshot(dir.path(), |built| {
        std::fs::remove_file(
            built
                .repo
                .path
                .join("tests/__snapshots__/minimal/files/pyproject.toml"),
        )
        .unwrap();
    })
    .failure();
    assert_eq!(output.error_code(), "tpl::testing::snapshot_read");
}

#[test]
fn a_snapshot_file_that_the_manifest_does_not_list_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let output = corrupted_snapshot(dir.path(), |built| {
        built.repo.write(
            "tests/__snapshots__/minimal/files/smuggled.txt",
            "never rendered\n",
        );
    })
    .failure();
    assert_eq!(output.error_code(), "tpl::testing::snapshot_read");
}

#[test]
fn a_snapshot_file_edited_without_its_digest_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    // Same length, different bytes: only the digest catches this one, which is
    // why the manifest records one rather than a size alone.
    let output = corrupted_snapshot(dir.path(), |built| {
        built.repo.write(
            "tests/__snapshots__/minimal/files/pyproject.toml",
            "name = \"OTHER\"\n",
        );
    })
    .failure();
    assert_eq!(output.error_code(), "tpl::testing::snapshot_read");
    assert!(output.stdout.contains("digest"), "{}", output.stdout);
}

#[test]
fn a_snapshot_file_of_the_wrong_length_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let output = corrupted_snapshot(dir.path(), |built| {
        built.repo.write(
            "tests/__snapshots__/minimal/files/pyproject.toml",
            "name = \"thing\"\nand more\n",
        );
    })
    .failure();
    assert_eq!(output.error_code(), "tpl::testing::snapshot_read");
}

#[test]
fn a_snapshot_with_an_unreadable_manifest_says_how_to_recover() {
    let dir = tempfile::tempdir().unwrap();
    let output = corrupted_snapshot(dir.path(), |built| {
        built
            .repo
            .write("tests/__snapshots__/minimal/MANIFEST", "not a manifest\n");
    })
    .failure();
    assert_eq!(output.error_code(), "tpl::testing::snapshot_read");
    assert!(
        output.stdout.contains("--write"),
        "the help says how to re-record: {}",
        output.stdout
    );
}

#[test]
fn write_over_a_changed_snapshot_reports_it_as_updated() {
    let dir = tempfile::tempdir().unwrap();
    let built = template(
        dir.path(),
        &[(
            "tests/minimal.toml",
            "snapshot = true\n[answers]\nproject_name = \"thing\"\n",
        )],
    );
    run(&built, &["--write"]).success();
    built.repo.commit_all("test: record");

    built.repo.write(
        "template/pyproject.toml.jinja",
        "name = \"{{ project_name }}\"\nnew = 1\n",
    );
    built.repo.commit_all("feat: change the template");

    let output = run(&built, &["--json", "--write"]).success();
    assert_eq!(
        output.json()["cases"][0]["snapshot"],
        "updated",
        "replacing an existing snapshot is not the same event as creating one"
    );
    assert_eq!(output.json()["summary"]["snapshotsWritten"], 1);
}

#[test]
fn contains_naming_a_file_the_template_does_not_render_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let built = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\nproject_name = \"a\"\n\n[expect.contains]\n\"absent.txt\" = \"x\"\n",
        )],
    );

    let output = run(&built, &["--json"]).code(1);
    let failure = &output.json()["cases"][0]["failures"][0];
    assert_eq!(failure["kind"], "containsMissingFile");
    assert_eq!(failure["path"], "absent.txt");
}

#[test]
fn lacks_naming_a_file_the_template_does_not_render_fails() {
    // A vacuous pass is exactly the bug class `lacks` exists to catch: "this
    // file does not mention X" must not go green because the file never
    // rendered at all.
    let dir = tempfile::tempdir().unwrap();
    let built = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\nproject_name = \"a\"\n\n[expect.lacks]\n\"absent.txt\" = \"x\"\n",
        )],
    );

    let output = run(&built, &["--json"]).code(1);
    let failure = &output.json()["cases"][0]["failures"][0];
    assert_eq!(failure["kind"], "lacksMissingFile");
    assert_eq!(failure["path"], "absent.txt");
}

#[test]
fn contains_cannot_look_inside_a_file_that_is_not_text() {
    let dir = tempfile::tempdir().unwrap();
    let built = Template::minimal(dir.path(), "name = \"bin\"\n", &[]);
    std::fs::create_dir_all(built.repo.path.join("template")).unwrap();
    // Genuinely not UTF-8: a lone 0xff is not a valid encoding of anything.
    // `[0, 1, 2, 3]` would decode fine as control characters and only report a
    // missing substring, which is a different failure.
    std::fs::write(
        built.repo.path.join("template/logo.png"),
        [0xffu8, 0xfe, 0, 1],
    )
    .unwrap();
    built
        .repo
        .write("tests/c.toml", "[expect.contains]\n\"logo.png\" = \"x\"\n");
    built.repo.commit_all("test: a binary file");

    let output = run(&built, &["--json"]).code(1);
    assert_eq!(
        output.json()["cases"][0]["failures"][0]["kind"],
        "containsNotUtf8"
    );

    run(&built, &[])
        .code(1)
        .says("`logo.png` is not text, so `contains` cannot look in it");
}

#[test]
fn lacks_cannot_look_inside_a_file_that_is_not_text() {
    let dir = tempfile::tempdir().unwrap();
    let built = Template::minimal(dir.path(), "name = \"bin\"\n", &[]);
    std::fs::create_dir_all(built.repo.path.join("template")).unwrap();
    // Genuinely not UTF-8: a lone 0xff is not a valid encoding of anything.
    // `[0, 1, 2, 3]` would decode fine as control characters and only report a
    // missing substring, which is a different failure.
    std::fs::write(
        built.repo.path.join("template/logo.png"),
        [0xffu8, 0xfe, 0, 1],
    )
    .unwrap();
    built
        .repo
        .write("tests/c.toml", "[expect.lacks]\n\"logo.png\" = \"x\"\n");
    built.repo.commit_all("test: a binary file");

    let output = run(&built, &["--json"]).code(1);
    assert_eq!(
        output.json()["cases"][0]["failures"][0]["kind"],
        "lacksNotUtf8"
    );

    run(&built, &[])
        .code(1)
        .says("`logo.png` is not text, so `lacks` cannot look in it");
}

/// The snapshot goes to the working tree, so the working tree can refuse it.
/// The diagnostic has to name the path and what the OS said, because "could
/// not write the snapshot" alone leaves the reader guessing at permissions.
#[cfg(unix)]
#[test]
fn a_snapshot_that_cannot_be_written_names_the_path_and_the_reason() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let built = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "snapshot = true\n[answers]\nproject_name = \"a\"\n",
        )],
    );

    let tests_dir = built.repo.path.join("tests");
    let original = std::fs::metadata(&tests_dir).unwrap().permissions();
    let mut locked = original.clone();
    locked.set_mode(0o500);
    std::fs::set_permissions(&tests_dir, locked).unwrap();

    let output = run(&built, &["--json", "--write"]).failure();

    // Restore before asserting, or a failure here leaves an undeletable
    // temporary directory behind.
    std::fs::set_permissions(&tests_dir, original).unwrap();

    assert_eq!(output.error_code(), "tpl::testing::snapshot_write");
    assert!(
        output.stdout.contains("__snapshots__"),
        "the help names the path: {}",
        output.stdout
    );
}

#[test]
fn a_missing_file_points_at_the_closest_path_the_template_did_render() {
    let dir = tempfile::tempdir().unwrap();
    let built = template(
        dir.path(),
        &[(
            "tests/c.toml",
            "[answers]\nproject_name = \"a\"\n\n[expect]\nfiles = [\"pyproject.tml\"]\n",
        )],
    );

    let output = run(&built, &["--json"]).code(1);
    let failure = &output.json()["cases"][0]["failures"][0];
    assert_eq!(failure["kind"], "missingFile");
    assert_eq!(
        failure["closest"], "pyproject.toml",
        "a typo in a case is a typo, and saying so beats listing the whole tree"
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_in_the_tests_directory_is_not_a_case() {
    let dir = tempfile::tempdir().unwrap();
    let built = template(
        dir.path(),
        &[("tests/real.toml", "[answers]\nproject_name = \"a\"\n")],
    );
    std::os::unix::fs::symlink("real.toml", built.repo.path.join("tests/link.toml")).unwrap();
    built.repo.commit_all("test: a symlink");

    let output = run(&built, &["--json"]).success();
    assert_eq!(
        output.json()["summary"]["total"],
        1,
        "a symlink is not a second case"
    );
}

#[test]
fn a_file_with_no_extension_is_not_a_case() {
    let dir = tempfile::tempdir().unwrap();
    let built = template(
        dir.path(),
        &[
            ("tests/real.toml", "[answers]\nproject_name = \"a\"\n"),
            ("tests/NOTES", "scratch, not a case\n"),
        ],
    );

    let output = run(&built, &["--json"]).success();
    assert_eq!(output.json()["summary"]["total"], 1);
}

#[test]
fn a_tests_directory_holding_no_cases_is_the_same_as_having_none() {
    let dir = tempfile::tempdir().unwrap();
    let built = template(
        dir.path(),
        &[("tests/README.md", "# how to write a case\n")],
    );

    let output = run(&built, &["--json"]).failure();
    assert_eq!(output.error_code(), "tpl::testing::no_tests");
}

/// Every failure kind, rendered for a person rather than a script.
///
/// The JSON shape is pinned case by case above; this pins the prose, because
/// the text output is what an author actually reads when their suite goes red
/// and a `kind` string tells them nothing.
#[test]
fn the_human_output_explains_every_kind_of_failure() {
    let dir = tempfile::tempdir().unwrap();
    let built = template(
        dir.path(),
        &[
            (
                "tests/missing.toml",
                "[answers]\nproject_name = \"a\"\n\n[expect]\nfiles = [\"pyproject.tml\"]\n",
            ),
            (
                "tests/unexpected.toml",
                "[answers]\nwith_ci = true\n\n[expect]\nabsent = [\"ci.yml\"]\n",
            ),
            (
                "tests/substring.toml",
                "[answers]\nproject_name = \"a\"\n\n[expect.contains]\n\"pyproject.toml\" = \"nowhere\"\n",
            ),
            (
                "tests/nofile.toml",
                "[answers]\nproject_name = \"a\"\n\n[expect.contains]\n\"absent.txt\" = \"x\"\n",
            ),
            (
                "tests/forbidden.toml",
                "[answers]\nproject_name = \"a\"\n\n[expect.lacks]\n\"pyproject.toml\" = \"a\"\n",
            ),
            (
                "tests/wrongcode.toml",
                "[answers]\nwith_ci = \"not a boolean\"\n\n[expect]\nerror = \"tpl::render::collision\"\n",
            ),
            (
                "tests/noerror.toml",
                "[answers]\nproject_name = \"a\"\n\n[expect]\nerror = \"tpl::eval::wrong_type\"\n",
            ),
            (
                "tests/blewup.toml",
                "[answers]\nwith_ci = \"not a boolean\"\n",
            ),
        ],
    );

    run(&built, &[])
        .code(1)
        // Every case is named, and every failure says what to do about it.
        .says("missing file      pyproject.tml")
        .says("the template rendered `pyproject.toml`")
        .says("unexpected file   ci.yml")
        .says("`pyproject.toml` does not contain: nowhere")
        .says("named by `contains`")
        .says("`pyproject.toml` contains: a")
        .says("expected tpl::render::collision, got")
        .says("expected the render to fail with tpl::eval::wrong_type, but it succeeded")
        .says("the render failed:")
        .says("add `error = \"tpl::eval::wrong_type\"` if that is the point of the case")
        .says("0 passed, 8 failed");
}

#[test]
fn the_human_output_of_a_snapshot_difference_says_how_to_re_record_it() {
    let dir = tempfile::tempdir().unwrap();
    let built = template(
        dir.path(),
        &[(
            "tests/minimal.toml",
            "snapshot = true\n[answers]\nproject_name = \"thing\"\n",
        )],
    );
    run(&built, &["--write"]).success();
    built.repo.commit_all("test: record");

    built.repo.write(
        "template/pyproject.toml.jinja",
        "name = \"{{ project_name }}\"\nnew = 1\n",
    );
    built.repo.commit_all("feat: change the template");

    // Without -v: what changed, but not how.
    run(&built, &[])
        .code(1)
        .says("snapshot differs (1 file)")
        .says("modified pyproject.toml")
        .says("re-record with `git tpl test --write`")
        .silent_about("+new = 1");

    // With -v: the hunks too. A large rendering would otherwise bury the list
    // of what changed under the change itself.
    run(&built, &["-v"]).code(1).says("+new = 1");
}

#[test]
fn the_human_output_reports_what_write_did_to_each_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let built = template(
        dir.path(),
        &[(
            "tests/minimal.toml",
            "snapshot = true\n[answers]\nproject_name = \"thing\"\n",
        )],
    );

    run(&built, &["--write"])
        .success()
        .says("snapshot written")
        .says("1 snapshot(s) recorded, 0 unchanged");
    built.repo.commit_all("test: record");

    run(&built, &["--write"])
        .success()
        .says("snapshot unchanged")
        .says("0 snapshot(s) recorded, 1 unchanged");

    run(&built, &[]).success().says("snapshot ok");
}

#[test]
fn a_case_trusts_a_remote_data_source_by_default() {
    let dir = tempfile::tempdir().unwrap();
    // The source is never reachable. What is under test is that an omitted
    // `trust` decides without a prompt — so the failure is the fetch, not a
    // refusal, and certainly not a hang waiting for an answer nobody can give.
    let built = Template::minimal(
        dir.path(),
        r#"
name = "remote-data"

[data.things]
source = "https://127.0.0.1:1/things.toml"
"#,
        &[("a.txt.jinja", "{{ data.things.name }}\n")],
    );
    built.repo.write("tests/c.toml", "[answers]\n");
    built.repo.commit_all("test: a remote source");

    let output = run(&built, &["--json"]).code(1);
    let failure = &output.json()["cases"][0]["failures"][0];
    assert_eq!(failure["kind"], "unexpectedError");
    assert_ne!(
        failure["code"], "tpl::data::untrusted",
        "trust defaults to true, so the source is reached rather than refused"
    );
}

#[test]
fn a_case_with_trust_false_refuses_a_remote_data_source() {
    let dir = tempfile::tempdir().unwrap();
    // Unlike the default-trust case above, this source does not even need to
    // be reachable in principle: `trust = false` refuses it before anything
    // touches the network, which is exactly what makes the refused path
    // testable at all without a TTY or a live host to refuse a connection.
    let built = Template::minimal(
        dir.path(),
        r#"
name = "remote-data"

[data.things]
source = "https://127.0.0.1:1/things.toml"
"#,
        &[("a.txt.jinja", "{{ data.things.name }}\n")],
    );
    built.repo.write(
        "tests/c.toml",
        "trust = false\n\n[expect]\nerror = \"tpl::data::untrusted\"\n",
    );
    built.repo.commit_all("test: an untrusted remote source");

    run(&built, &["--json"]).success();
}

#[test]
fn a_case_fails_when_an_answer_names_no_question() {
    let dir = tempfile::tempdir().unwrap();
    // `projct_name` is the same deliberate typo `typos.toml` already
    // allowlists for `strict_answers_refuses_a_key_that_names_no_question`
    // in `tests/render.rs` — the repro from #135: a case's `[answers]` has
    // no `--strict-answers` to catch this, so it must be caught
    // unconditionally. See ADR-029.
    let built = template(
        dir.path(),
        &[("tests/c.toml", "[answers]\nprojct_name = \"typo\"\n")],
    );

    let output = run(&built, &["--json"]).code(1);
    let failure = &output.json()["cases"][0]["failures"][0];
    assert_eq!(failure["code"], "tpl::answers::unknown_key");
    assert!(
        failure["message"]
            .as_str()
            .expect("message")
            .contains("projct_name"),
        "the message should name the offending key"
    );
}

#[test]
fn a_case_with_only_real_questions_in_its_answers_is_unaffected() {
    let dir = tempfile::tempdir().unwrap();
    let built = template(
        dir.path(),
        &[("tests/c.toml", "[answers]\nproject_name = \"thing\"\n")],
    );

    run(&built, &["--json"]).success();
}

#[test]
fn git_tpl_test_no_longer_accepts_a_trust_flag() {
    let dir = tempfile::tempdir().unwrap();
    let built = template(dir.path(), &[("tests/c.toml", "[answers]\n")]);

    run(&built, &["--trust"])
        .failure()
        .says("unexpected argument");
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
