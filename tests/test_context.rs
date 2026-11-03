//! Context assertions exercise the actual questionnaire and lazy render session, with real Git.
mod common;

use common::{Template, tpl};

const MANIFEST: &str = r#"
name = "context-tests"
[questions.feature_enabled]
type = "boolean"
default = false
[questions.feature_label]
type = "string"
when = "{{ feature_enabled }}"
default = "Default label"
[questions.fallback]
type = "string"
when = "{{ feature_enabled }}"
default = "fallback"
default_when_skipped = true
[computed]
label = "{{ feature_label | default('disabled') }}"
nothing = "{{ none }}"
empty = ""
options = { enabled = false, items = [1, 2] }
[data.payload]
source = "data/payload.json"
[data.broken]
source = "data/missing.json"
"#;

fn fixture(parent: &std::path::Path) -> Template {
    let template = Template::minimal(parent, MANIFEST, &[("constant.txt", "unchanged\n")]);
    template.repo.write(
        "data/payload.json",
        r#"{"optional":null,"empty":"","flag":false,"nested":[null,{"key":null}]}"#,
    );
    template
}

#[test]
fn context_assertions_cover_skips_defaults_explicit_answers_and_native_null_without_output_changes()
{
    let dir = tempfile::tempdir().unwrap();
    let template = fixture(dir.path());
    template.repo.write("tests/disabled.toml", r#"
[answers]
feature_label = "ignored while skipped"
[expect]
files = ["constant.txt"]
[expect.context]
absent = ["feature_label", "unknown"]
present = ["nothing", "empty", "feature_enabled", "fallback"]
expressions = ["{{ nothing is defined and nothing is none }}", "{{ data.payload.nested[1].key is none }}"]
[expect.context.equals]
feature_enabled = false
label = "disabled"
fallback = "fallback"
empty = ""
options = { enabled = false, items = [1, 2] }
"#);
    template.repo.write(
        "tests/default.toml",
        r#"
[answers]
feature_enabled = true
[expect.context.equals]
feature_label = "Default label"
label = "Default label"
"#,
    );
    template.repo.write(
        "tests/explicit.yaml",
        r#"
answers:
  feature_enabled: true
  feature_label: Custom
expect:
  context:
    present: [nothing]
    absent: [data.payload.missing]
    equals:
      feature_label: Custom
      label: Custom
      nothing: null
      data.payload.optional: null
      data.payload.empty: ""
      data.payload.flag: false
      data.payload.nested: [null, {key: null}]
"#,
    );
    let result = tpl(&template.repo, &["test", "--json", "--skip-commands"]).success();
    assert_eq!(result.json()["summary"]["passed"], 3);
    // The unreferenced broken data declaration must not be acquired by inspection.
    assert_eq!(result.json()["summary"]["failed"], 0);
    template.repo.write(
        "tests/default.toml",
        r#"
[answers]
feature_enabled = true
[expect.context.equals]
label = "broken expectation"
"#,
    );
    let failed = tpl(&template.repo, &["test", "--json"]).failure();
    assert_eq!(failed.json()["summary"]["failed"], 1);
    let failure = &failed.json()["cases"][0]["failures"][0];
    assert_eq!(failure["kind"], "contextMismatch");
    assert_eq!(failure["actual"], "Default label");
    assert_eq!(failure["expected"], "broken expectation");
}

#[test]
fn reports_distinguish_missing_null_false_empty_and_nonboolean_expression_results() {
    let dir = tempfile::tempdir().unwrap();
    let template = fixture(dir.path());
    template.repo.write(
        "tests/fail.json",
        r#"{
      "expect":{"context":{
        "present":["missing"], "absent":["data.payload.optional"],
        "equals":{"feature_label":null,"nothing":"","feature_enabled":"false","empty":false},
        "expressions":["{{ false }}","{{ 'true' }}","{{ none }}"]
      }}
    }"#,
    );
    let result = tpl(&template.repo, &["test", "--json"]).failure();
    let json = result.json();
    let failures = json["cases"][0]["failures"]
        .as_array()
        .unwrap_or_else(|| panic!("{json:#}"));
    assert_eq!(failures.len(), 9);
    assert_eq!(failures[0]["kind"], "contextMissing");
    assert_eq!(failures[0]["actualPresent"], false);
    assert!(failures[0].get("expected").is_none());
    assert_eq!(failures[1]["kind"], "contextUnexpected");
    assert_eq!(failures[1]["actualPresent"], true);
    assert!(failures[1]["actual"].is_null());
    let missing_null = failures
        .iter()
        .find(|failure| failure["path"] == "feature_label")
        .unwrap();
    assert!(missing_null.get("expected").unwrap().is_null());
    assert!(missing_null.get("actual").is_none());
    let human = tpl(&template.repo, &["test"]).failure();
    assert!(human.stderr.contains("<absent>") || human.stdout.contains("<absent>"));
}

#[test]
fn recording_does_not_evaluate_assertion_only_demands_and_absence_checks_do() {
    let dir = tempfile::tempdir().unwrap();
    let template = fixture(dir.path());
    template.repo.write(
        "tests/record.toml",
        r#"
snapshot = true
[expect.context]
absent = ["data.broken"]
"#,
    );
    tpl(&template.repo, &["test", "--write"]).success();
    let json = tpl(&template.repo, &["test", "--json"]).failure().json();
    assert_eq!(json["cases"][0]["failures"][0]["kind"], "unexpectedError");
    assert_eq!(json["cases"][0]["failures"][0]["code"], "tpl::data::load");
}

#[test]
fn broken_question_conditions_and_computations_fail_even_when_the_snapshot_stays_identical() {
    let dir = tempfile::tempdir().unwrap();
    let template = fixture(dir.path());
    template.repo.write(
        "tests/regression.toml",
        r#"
snapshot = true
[expect.context]
absent = ["feature_label"]
[expect.context.equals]
label = "disabled"
"#,
    );
    tpl(&template.repo, &["test", "--write"]).success();
    tpl(&template.repo, &["test"]).success();
    template.repo.write(
        "template.toml",
        &MANIFEST.replace("when = \"{{ feature_enabled }}\"", "when = \"{{ true }}\""),
    );
    let broken = tpl(&template.repo, &["test", "--json"]).failure().json();
    assert_eq!(broken["cases"][0]["snapshot"], "compared");
    let failures = broken["cases"][0]["failures"].as_array().unwrap();
    assert_eq!(failures[0]["kind"], "contextUnexpected");
    assert_eq!(failures[1]["kind"], "contextMismatch");
    template.repo.write(
        "template.toml",
        &MANIFEST.replace("default('disabled')", "default('broken')"),
    );
    let broken = tpl(&template.repo, &["test", "--json"]).failure().json();
    assert_eq!(broken["cases"][0]["snapshot"], "compared");
    assert_eq!(broken["cases"][0]["failures"][0]["kind"], "contextMismatch");
}

#[test]
fn assertion_only_data_access_obeys_case_trust_and_computed_absence_is_not_lazy_absence() {
    let dir = tempfile::tempdir().unwrap();
    let template = fixture(dir.path());
    template.repo.write(
        "template.toml",
        &format!("{MANIFEST}\n[data.remote]\nsource = 'https://example.invalid/data.json'\n"),
    );
    template.repo.write(
        "tests/refused.toml",
        r#"
trust = false
[expect.context]
present = ["data.remote"]
"#,
    );
    let refused = tpl(&template.repo, &["test", "--json"]).failure().json();
    assert_eq!(
        refused["cases"][0]["failures"][0]["code"],
        "tpl::data::untrusted"
    );
    template.repo.write(
        "tests/refused.toml",
        r#"
trust = false
[expect.context]
absent = ["label"]
expressions = ["{{ true or data.remote }}"]
"#,
    );
    let resolved = tpl(&template.repo, &["test", "--json"]).failure().json();
    assert_eq!(
        resolved["cases"][0]["failures"][0]["kind"],
        "contextUnexpected"
    );
    assert_eq!(resolved["cases"][0]["failures"][0]["actual"], "disabled");
}

#[test]
fn sandboxed_assertions_and_command_opt_out_use_the_same_context() {
    let dir = tempfile::tempdir().unwrap();
    let template = fixture(dir.path());
    template.repo.write(
        "tests/sandbox.toml",
        r#"
[commands]
rendered = ["git --version"]
[expect.context]
expressions = ["{{ true or data.broken.value }}"]
[expect.context.equals]
label = "disabled"
"#,
    );
    tpl(&template.repo, &["test"]).success();
    tpl(&template.repo, &["test", "--skip-commands"]).success();
}

#[test]
fn malformed_context_expectations_are_rejected_before_rendering() {
    for body in [
        "[expect.context]\npresent = ['x']\nabsent = ['x']",
        "[expect.context]\nequals = []",
        "[expect.context]\npresent = ['data']",
        "[expect.context]\npresent = ['x..y']",
        "[expect.context]\nexpressions = ['true']",
        "[expect.context]\npresnt = ['x']",
        "[expect]\nerror = 'tpl::eval::unanswered'\n[expect.context]\npresent = ['x']",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let template = fixture(dir.path());
        template.repo.write("tests/bad.toml", body);
        let result = tpl(&template.repo, &["test", "--json"]).failure();
        assert_eq!(result.error_code(), "tpl::testing::case_shape", "{body}");
    }
}

#[test]
fn null_context_values_are_defined_but_null_answers_do_not_make_questions_nullable() {
    let dir = tempfile::tempdir().unwrap();
    let template = fixture(dir.path());
    let context = tpl(
        &template.repo,
        &["context", "--dirty", "--defaults", "--json"],
    )
    .success()
    .json();
    assert!(context["computed"].get("nothing").unwrap().is_null());
    template.repo.write("tests/null-answer.json", r#"{"answers":{"feature_label":null,"feature_enabled":true},"expect":{"error":"tpl::eval::wrong_type"}}"#);
    tpl(&template.repo, &["test"]).success();
}
