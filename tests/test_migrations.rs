//! Declarative upgrades exercise real template and project histories, never a simulated merge.

mod common;

use common::{Template, tpl};
use std::path::Path;

const OLD: &str = "# Project\n\nTitle\n\nLocal: default\n\nFooter\n";
const LOCAL: &str = "# Project\n\nTitle\n\nLocal: customised\n\nFooter\n";
const MANIFEST: &str = r#"
name = "migration-tests"
[questions.name]
type = "string"
default = "demo"
"#;

fn template(parent: &Path) -> Template {
    let template = Template::minimal(
        parent,
        MANIFEST,
        &[("README.md", OLD), ("obsolete.txt", "old\n")],
    );
    template.repo.git(&["tag", "v1"]);
    std::fs::remove_file(template.repo.path.join("template/README.md")).unwrap();
    template.repo.write("template/docs/README.md", OLD);
    template.repo.write(
        "migrations/010-readme.toml",
        r#"
message = "Moved README for {{ name }}"
[[moves]]
from = "README.md"
to = "docs/README.md"
"#,
    );
    template.repo.commit_all("move README");
    template.repo.git(&["tag", "v2"]);
    template
}

fn case(template: &Template, name: &str, body: &str) {
    template.repo.write(&format!("tests/{name}.toml"), body);
}

fn run(template: &Template, args: &[&str]) -> common::Output {
    let mut command = vec!["test", "--json"];
    command.extend_from_slice(args);
    tpl(&template.repo, &command)
}

#[test]
fn context_assertions_inspect_the_destination_update_with_replayed_answers() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path());
    template.repo.write(
        "template.toml",
        &format!("{MANIFEST}\n[computed]\ndestination_only = '{{{{ name }}}}'\n"),
    );
    case(
        &template,
        "context",
        r#"
snapshot = true
[migration]
from = "v1"
[migration.answers]
name = "recorded"
[expect.context.equals]
name = "recorded"
destination_only = "recorded"
"#,
    );
    run(&template, &["--skip-commands"]).failure();
    run(&template, &["--write"]).success();
    run(&template, &["--skip-commands"]).success();
    case(
        &template,
        "context",
        r#"
snapshot = true
[migration]
from = "v1"
[expect.context.equals]
destination_only = "wrong"
"#,
    );
    let result = run(&template, &[]).failure().json();
    let failure = &result["cases"][0]["failures"][0];
    assert_eq!(failure["kind"], "migration");
    assert_eq!(failure["phase"], "assertions");
    assert_eq!(failure["failure"]["kind"], "contextMismatch");
    assert_eq!(failure["failure"]["actual"], "demo");
    run(&template, &["--write"]).success();
}

#[test]
fn a_move_preserves_committed_customisations_and_project_owned_files_without_touching_the_caller() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path());
    case(
        &template,
        "move",
        &format!(
            r#"
[migration]
from = "v1"
absent = ["obsolete.txt"]
[migration.files]
"README.md" = {LOCAL:?}
"local.txt" = "project owned\n"
[expect]
files = ["docs/README.md", "local.txt"]
absent = ["README.md", "obsolete.txt", ".config/git.tpl.toml"]
migrations = ["migrations/010-readme.toml"]
[expect.contains]
"docs/README.md" = "Local: customised"
"local.txt" = "project owned"
[expect.messages]
"migrations/010-readme.toml" = ["Moved README", "demo"]
"#
        ),
    );
    let head = template.repo.git(&["rev-parse", "HEAD"]);
    let index = std::fs::read(template.repo.path.join(".git/index")).unwrap();
    let status = template.repo.git(&["status", "--porcelain"]);
    let result = run(&template, &["--skip-commands"]).success();
    assert_eq!(result.json()["summary"]["passed"], 1);
    assert_eq!(template.repo.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(
        std::fs::read(template.repo.path.join(".git/index")).unwrap(),
        index
    );
    assert_eq!(template.repo.git(&["status", "--porcelain"]), status);
    assert_eq!(
        std::fs::read_to_string(template.repo.path.join("template/docs/README.md")).unwrap(),
        OLD
    );
}

#[test]
fn a_dirty_destination_runs_chained_migrations_but_intermediate_cases_use_their_own_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path());
    std::fs::remove_file(template.repo.path.join("template/docs/README.md")).unwrap();
    template.repo.write("template/guide.md", OLD);
    template.repo.write(
        "migrations/020-guide.toml",
        r#"
[[moves]]
from = "docs/README.md"
to = "guide.md"
"#,
    );
    case(
        &template,
        "jump",
        r#"
[migration]
from = "v1"
[expect]
files = ["guide.md"]
absent = ["README.md", "docs/README.md"]
migrations = ["migrations/010-readme.toml", "migrations/020-guide.toml"]
"#,
    );
    case(
        &template,
        "intermediate",
        r#"
[migration]
from = "v1"
to = "v2"
[expect]
files = ["docs/README.md"]
absent = ["guide.md"]
migrations = ["migrations/010-readme.toml"]
"#,
    );
    case(
        &template,
        "later",
        r#"
[migration]
from = "v2"
[expect]
migrations = ["migrations/020-guide.toml"]
"#,
    );
    assert_eq!(run(&template, &[]).success().json()["summary"]["passed"], 3);
}

#[test]
fn move_and_content_changes_merge_with_nonoverlapping_local_changes() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path());
    template.repo.write(
        "template/docs/README.md",
        &format!("{OLD}\nNew template footer\n"),
    );
    case(
        &template,
        "edited",
        &format!(
            r#"
[migration]
from = "v1"
[migration.files]
"README.md" = {LOCAL:?}
[expect.contains]
"docs/README.md" = ["Local: customised", "New template footer"]
"#
        ),
    );
    run(&template, &[]).success();
}

#[test]
fn order_and_message_failures_identify_migrations_and_output_mismatches_identify_paths() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path());
    case(
        &template,
        "wrong",
        r#"
[migration]
from = "v1"
[expect]
migrations = []
[expect.contains]
"docs/README.md" = "missing text"
[expect.messages]
"migrations/010-readme.toml" = "wrong message"
"#,
    );
    let output = run(&template, &[]).failure();
    let json = output.json();
    let failures = json["cases"][0]["failures"].as_array().unwrap();
    assert_eq!(failures.len(), 3);
    assert_eq!(failures[0]["migrations"][0], "migrations/010-readme.toml");
    assert_eq!(failures[0]["failure"]["path"], "docs/README.md");
    assert_eq!(failures[1]["failure"]["assertion"], "expect.migrations");
    assert_eq!(failures[2]["failure"]["path"], "migrations/010-readme.toml");
    let human = tpl(&template.repo, &["test"]).failure();
    assert!(
        human.stderr.contains("migrations/010-readme.toml"),
        "{}",
        human.stderr
    );
    assert!(human.stderr.contains("docs/README.md"));
}

#[test]
fn merge_conflicts_are_case_failures_and_do_not_stop_other_cases() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path());
    template.repo.write(
        "template/docs/README.md",
        &OLD.replace("default", "template edit"),
    );
    case(
        &template,
        "conflict",
        &format!(
            r#"
[migration]
from = "v1"
[migration.files]
"README.md" = {LOCAL:?}
"#
        ),
    );
    case(&template, "ordinary", "");
    let json = run(&template, &[]).failure().json();
    assert_eq!(json["summary"]["passed"], 1);
    assert_eq!(json["summary"]["failed"], 1);
    assert_eq!(json["cases"][0]["failures"][0]["phase"], "merge");
    assert_eq!(
        json["cases"][0]["failures"][0]["failure"]["path"],
        "README.md"
    );
}

#[test]
fn snapshots_record_the_merged_project_without_commands_expectations_or_attachment_bookkeeping() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path());
    case(
        &template,
        "record",
        &format!(
            r#"
snapshot = true
[migration]
from = "v1"
[migration.files]
"README.md" = {LOCAL:?}
[expect]
files = ["intentionally-wrong"]
migrations = []
[commands]
before = ["does-not-exist"]
rendered = ["does-not-exist"]
finally = ["does-not-exist"]
"#
        ),
    );
    let output = run(&template, &["--write"]).success();
    assert_eq!(output.json()["summary"]["commandsRun"], 0);
    let snapshot = template.repo.path.join("tests/__snapshots__/record");
    assert_eq!(
        std::fs::read_to_string(snapshot.join("files/docs/README.md")).unwrap(),
        LOCAL
    );
    assert!(!snapshot.join("files/.config/git.tpl.toml").exists());
    case(
        &template,
        "record",
        &format!(
            r#"
snapshot = true
[migration]
from = "v1"
[migration.files]
"README.md" = {LOCAL:?}
"#
        ),
    );
    run(&template, &["--skip-commands"]).success();
    template.repo.commit_all("record migration snapshot");
    run(&template, &["--ref", "main"]).success();
}

#[test]
fn old_answers_are_recorded_and_target_overrides_are_strict_without_rejecting_removed_questions() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path());
    template.repo.write(
        "template.toml",
        "name = 'migration-tests'\n[questions.new]\ntype = 'string'\ndefault = 'target'\n",
    );
    template.repo.write("template/new.txt.jinja", "{{ new }}");
    case(
        &template,
        "answers",
        r#"
[answers]
new = "override"
[migration]
from = "v1"
[migration.answers]
name = "old"
[expect.contains]
"new.txt" = "override"
"#,
    );
    case(
        &template,
        "strict",
        r#"
[answers]
typo = "bad"
[migration]
from = "v1"
[migration.answers]
[expect]
error = "tpl::answers::unknown_key"
"#,
    );
    run(&template, &[]).success();
}

#[test]
fn an_old_state_failure_cannot_satisfy_an_expected_destination_error() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path());
    case(
        &template,
        "old",
        r#"
[migration]
from = "v1"
[migration.answers]
typo = true
[expect]
error = "tpl::answers::unknown_key"
"#,
    );
    let json = run(&template, &[]).failure().json();
    assert_eq!(json["cases"][0]["failures"][0]["phase"], "init");
    assert_eq!(
        json["cases"][0]["failures"][0]["failure"]["kind"],
        "unexpectedError"
    );
}

#[test]
fn a_failed_move_carries_its_declaring_migration_and_keeps_its_diagnostic_code() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path());
    template.repo.write(
        "migrations/020-broken.toml",
        "[[moves]]\nfrom = 'missing'\nto = 'target'\n",
    );
    case(
        &template,
        "expected",
        r#"
[migration]
from = "v1"
[expect]
error = "tpl::migration::move_source_missing"
"#,
    );
    run(&template, &[]).success();
    case(&template, "unexpected", "[migration]\nfrom = 'v1'\n");
    let json = run(&template, &[]).failure().json();
    let failure = &json["cases"][1]["failures"][0]["failure"];
    assert!(
        failure["message"]
            .as_str()
            .unwrap()
            .contains("migrations/020-broken.toml")
    );
    assert_eq!(failure["code"], "tpl::migration::move_source_missing");
}

#[test]
fn literal_and_file_backed_messages_use_the_destination_answers() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path());
    template.repo.write(
        "migrations/020-message.toml",
        "message_file = 'messages/upgrade.jinja'\n",
    );
    template
        .repo
        .write("messages/upgrade.jinja", "Upgrade {{ name }}\n");
    case(
        &template,
        "message",
        r#"
[answers]
name = "target"
[migration]
from = "v1"
[migration.answers]
name = "old"
[expect.messages]
"migrations/010-readme.toml" = "for target"
"migrations/020-message.toml" = "Upgrade target"
"#,
    );
    run(&template, &[]).success();
}

#[test]
fn migration_commands_share_the_project_repository_and_finally_runs_every_entry_after_failure() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path());
    // libgit2 reads XDG Git configuration rather than native Git's GIT_CONFIG_GLOBAL.
    // Reproduce Windows checkout defaults on every platform: commands must still see a clean project.
    let git_config = template.repo.config_home().join("git");
    std::fs::create_dir_all(&git_config).unwrap();
    std::fs::write(
        git_config.join("config"),
        "[core]\n\tautocrlf = true\n\teol = crlf\n",
    )
    .unwrap();
    case(
        &template,
        "commands",
        r#"
[migration]
from = "v1"
[commands]
before = ["git config test.setup yes"]
rendered = ["git ls-files --error-unmatch docs/README.md", "git config --get test.setup"]
after = ["git diff --exit-code"]
finally = ["git status --porcelain"]
"#,
    );
    assert_eq!(
        run(&template, &[]).success().json()["summary"]["commandsRun"],
        5
    );
    assert_eq!(
        run(&template, &["--skip-commands"]).success().json()["summary"]["commandsRun"],
        0
    );
    case(
        &template,
        "commands",
        r#"
[migration]
from = "missing-reference"
[commands]
finally = ["git nonexistent-subcommand", "git status --porcelain"]
"#,
    );
    let json = run(&template, &[]).failure().json();
    assert_eq!(json["summary"]["commandsRun"], 2);
    assert_eq!(json["cases"][0]["failures"].as_array().unwrap().len(), 2);
}

#[test]
fn a_destination_only_remote_data_declaration_uses_the_cases_trust_decision() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path());
    template.repo.write(
        "template.toml",
        &format!("{MANIFEST}\n[data.things]\nsource = 'https://127.0.0.1:1/things.toml'\n"),
    );
    template
        .repo
        .write("template/data.txt.jinja", "{{ data.things.name }}");
    case(
        &template,
        "refused",
        r#"
trust = false
[migration]
from = "v1"
[expect]
error = "tpl::data::untrusted"
"#,
    );
    run(&template, &[]).success();
}

#[test]
fn an_unchanged_template_still_records_changed_answers_without_reapplying_old_migrations() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path());
    case(
        &template,
        "unchanged",
        r#"
[answers]
name = "new answer without output changes"
[migration]
from = "v2"
to = "v2"
[migration.answers]
name = "old answer"
[expect]
migrations = []
files = ["docs/README.md"]
"#,
    );
    run(&template, &[]).success();
}

#[test]
fn missing_local_deletions_and_missing_migration_messages_are_actionable_case_failures() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path());
    case(
        &template,
        "deletion",
        r#"
[migration]
from = "v1"
absent = ["not-in-old-project"]
"#,
    );
    template.repo.write("migrations/020-silent.toml", "");
    case(
        &template,
        "messages",
        r#"
[migration]
from = "v1"
[expect.messages]
"migrations/020-silent.toml" = "required message"
"migrations/030-not-discovered.toml" = "required message"
"#,
    );
    let json = run(&template, &[]).failure().json();
    assert_eq!(json["cases"][0]["failures"][0]["phase"], "local-edits");
    assert!(
        json["cases"][0]["failures"][0]["failure"]["message"]
            .as_str()
            .unwrap()
            .contains("not-in-old-project")
    );
    assert_eq!(json["cases"][1]["failures"].as_array().unwrap().len(), 2);
}

#[test]
fn a_nondefault_render_root_is_resolved_independently_for_each_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path());
    template
        .repo
        .write("template.toml", &format!("root = 'output'\n{MANIFEST}"));
    template.repo.write("output/docs/README.md", OLD);
    case(
        &template,
        "root",
        r#"
[migration]
from = "v1"
[expect]
files = ["docs/README.md"]
absent = ["README.md"]
"#,
    );
    run(&template, &[]).success();
}

#[test]
fn inherited_rendering_and_composed_child_lock_changes_use_production_update() {
    let base_dir = tempfile::tempdir().unwrap();
    let base = Template::minimal(
        base_dir.path(),
        "name = 'base'\n",
        &[("base.txt", "inherited")],
    );
    let child_dir = tempfile::tempdir().unwrap();
    let child = Template::minimal(
        child_dir.path(),
        r#"
name = "child"
[questions.module_name]
type = "string"
[questions.module_path]
type = "string"
"#,
        &[("{{ module_path }}/module.txt.jinja", "{{ module_name }}")],
    );
    child.repo.git(&["tag", "child-v1"]);
    child.repo.write("template/second.txt", "new child file");
    child.repo.commit_all("change composed child");
    child.repo.git(&["tag", "child-v2"]);
    let dir = tempfile::tempdir().unwrap();
    let manifest = |reference| {
        format!(
            r#"
name = "parent"
[extends]
source = '{}'
rev = '{}'
[questions.modules]
type = "list"
default = ["one"]
[[compose]]
source = '{}'
ref = '{reference}'
from = "modules"
"#,
            base.source(),
            base.repo.rev_parse("HEAD"),
            child.source()
        )
    };
    let template = Template::minimal(dir.path(), &manifest("child-v1"), &[("old.txt", "leaf")]);
    template.repo.git(&["tag", "v1"]);
    template.repo.write("template.toml", &manifest("child-v2"));
    std::fs::remove_file(template.repo.path.join("template/old.txt")).unwrap();
    template.repo.write("template/new.txt", "leaf");
    template.repo.write(
        "migrations/010-leaf.toml",
        "[[moves]]\nfrom = 'old.txt'\nto = 'new.txt'\n",
    );
    case(
        &template,
        "composed",
        r#"
[migration]
from = "v1"
[expect]
files = ["base.txt", "new.txt", "modules/one/module.txt", "second.txt"]
absent = ["old.txt"]
[expect.contains]
"base.txt" = "inherited"
"modules/one/module.txt" = "one"
"#,
    );
    run(&template, &[]).success();
}

#[test]
fn migration_cases_parse_in_every_existing_format_and_reject_vacuous_combinations() {
    for (path, body) in [
        (
            "tests/a.toml",
            "[migration]\nfrom = 'v1'\n[expect]\nmigrations = []",
        ),
        (
            "tests/a.json",
            r#"{"migration":{"from":"v1"},"expect":{"migrations":[]}}"#,
        ),
        (
            "tests/a.yaml",
            "migration:\n  from: v1\nexpect:\n  migrations: []\n",
        ),
        (
            "tests/a.yml",
            "migration:\n  from: v1\nexpect:\n  migrations: []\n",
        ),
    ] {
        let case = tpl::ops::testing::Case::parse("a", path, body.as_bytes()).unwrap();
        assert_eq!(case.migration.unwrap().from, "v1");
        assert_eq!(case.expect.migrations, Some(vec![]));
    }
    for body in [
        "[expect]\nmigrations = []",
        "[migration]\nfrom = ''",
        "[migration]\nfrom = 'v1'\nform = 'typo'",
        "[migration]\nfrom = 'v1'\n[expect.messages]\n'migrations/a.toml' = []",
        "[migration]\nfrom = 'v1'\n[expect]\nerror = 'tpl::eval::wrong_type'\nmigrations = []",
    ] {
        assert!(
            tpl::ops::testing::Case::parse("a", "tests/a.toml", body.as_bytes()).is_err(),
            "{body}"
        );
    }
}

#[test]
fn declarative_project_edits_reject_unsafe_and_overlapping_paths() {
    for edits in [
        "[migration.files]\n'../escape' = 'bad'",
        "[migration.files]\n'.git/config' = 'bad'",
        "[migration.files]\n'.config/git.tpl.toml' = 'bad'",
        "[migration.files]\n'a' = 'bad'\n'a/b' = 'bad'",
        "absent = ['README.md']\n[migration.files]\n'README.md' = 'bad'",
    ] {
        let body = format!("[migration]\nfrom = 'v1'\n{edits}\n");
        assert!(
            tpl::ops::testing::Case::parse("bad", "tests/bad.toml", body.as_bytes()).is_err(),
            "{body}"
        );
    }
}

#[cfg(unix)]
#[test]
fn replacing_an_old_symlink_never_writes_through_it() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path());
    template.repo.git(&["checkout", "v1"]);
    std::os::unix::fs::symlink("README.md", template.repo.path.join("template/link")).unwrap();
    template.repo.commit_all("add old link");
    template.repo.git(&["tag", "with-link"]);
    case(
        &template,
        "link",
        r#"
[migration]
from = "with-link"
[migration.files]
link = "replacement"
[expect.contains]
link = "replacement"
"README.md" = "Local: default"
"#,
    );
    run(&template, &[]).success();
}
