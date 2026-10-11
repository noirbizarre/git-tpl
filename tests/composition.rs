//! Multiple separately pinned child repositories render into one project without executing template code.

mod common;

use common::{Repo, tpl};

fn child(name: &str) -> Repo {
    let child = Repo::init(name);
    child.write("template.toml", &format!(
        "name = '{name}'\n[questions.module_name]\ntype = 'string'\n[questions.module_path]\ntype = 'string'\n"
    ));
    child.write(
        "template/{{ module_path }}/module.txt.jinja",
        "{{ module_name }}\n",
    );
    child.commit_all("initial child template");
    child
}

fn parent(source: &Repo, integration: &Repo, source_ref: &str, integration_ref: &str) -> Repo {
    let parent = Repo::init("parent");
    parent.write("template.toml", &format!(
        "name = 'parent'\n[questions.sources]\ntype = 'list'\ndefault = []\n[questions.integrations]\ntype = 'list'\ndefault = []\n[[compose]]\nsource = '{}'\nref = '{source_ref}'\nfrom = 'sources'\n[[compose]]\nsource = '{}'\nref = '{integration_ref}'\nfrom = 'integrations'\n",
        source.path.display(), integration.path.display()
    ));
    parent.write("template/README.md", "This is the parent\n");
    parent.commit_all("initial parent template");
    parent
}

#[test]
fn zero_selected_modules_render_the_parent_without_reading_children() {
    let source = child("source");
    let integration = child("integration");
    let parent = parent(&source, &integration, "0000000", "0000000");
    let output = tempfile::tempdir().unwrap();
    let output = output.path().join("generated");
    tpl(
        &parent,
        &[
            "--json",
            "render",
            parent.path.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--defaults",
        ],
    )
    .success();
    assert_eq!(
        std::fs::read_to_string(output.join("README.md")).unwrap(),
        "This is the parent\n"
    );
}

#[test]
fn two_sources_and_an_integration_render_from_independent_revisions() {
    let source = child("source");
    let integration = child("integration");
    let source_ref = source.git(&["rev-parse", "HEAD"]);
    let integration_ref = integration.git(&["rev-parse", "HEAD"]);
    let parent = parent(&source, &integration, &source_ref, &integration_ref);
    let output = tempfile::tempdir().unwrap();
    let output = output.path().join("generated");
    tpl(
        &parent,
        &[
            "render",
            parent.path.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--answer",
            "sources=[\"jira\",\"confluence\"]",
            "--answer",
            "integrations=[\"codex\"]",
            "--defaults",
        ],
    )
    .success();
    for name in ["jira", "confluence", "codex"] {
        assert_eq!(
            std::fs::read_to_string(output.join(format!("modules/{name}/module.txt"))).unwrap(),
            format!("{name}\n")
        );
    }
}

#[test]
fn a_child_branch_is_resolved_for_a_project_free_render() {
    let source = child("source");
    let integration = child("integration");
    let integration_ref = integration.git(&["rev-parse", "HEAD"]);
    let parent = parent(&source, &integration, "main", &integration_ref);
    let output = tempfile::tempdir().unwrap();
    let output = output.path().join("generated");
    let result = tpl(
        &parent,
        &[
            "--json",
            "render",
            parent.path.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--answer",
            "sources=[\"jira\"]",
            "--defaults",
        ],
    )
    .success()
    .json();
    assert_eq!(result["composed"][0]["ref"], "main");
    assert_eq!(
        result["composed"][0]["commit"],
        source.git(&["rev-parse", "HEAD"])
    );
    assert!(!output.join(".config/git.tpl.toml").exists());
    assert_eq!(
        std::fs::read_to_string(output.join("modules/jira/module.txt")).unwrap(),
        "jira\n"
    );
}

#[test]
fn init_records_a_branch_to_follow_and_the_exact_child_commit_it_rendered() {
    let source = child("source");
    let integration = child("integration");
    let parent = parent(&source, &integration, "main", "main");
    let project = Repo::init("project");
    tpl(
        &project,
        &[
            "--json",
            "init",
            parent.path.to_str().unwrap(),
            "--answer",
            "sources=[\"jira\"]",
            "--defaults",
        ],
    )
    .success();
    let config = tpl::config::Config::load(&project.path).unwrap();
    assert_eq!(config.composed.len(), 1);
    assert_eq!(config.composed[0].from, "sources");
    assert_eq!(config.composed[0].reference, "main");
    assert_eq!(
        config.composed[0].commit,
        source.git(&["rev-parse", "HEAD"])
    );
    let status = tpl(&project, &["--json", "status"]).success().json();
    assert_eq!(
        status["renderedComposed"][0]["commit"],
        config.composed[0].commit
    );
}

#[test]
fn moving_a_child_branch_with_identical_output_records_a_new_commit_and_lock() {
    let source = child("source");
    let integration = child("integration");
    let parent = parent(&source, &integration, "main", "main");
    let project = Repo::init("project");
    tpl(
        &project,
        &[
            "--json",
            "init",
            parent.path.to_str().unwrap(),
            "--answer",
            "sources=[\"jira\"]",
            "--defaults",
        ],
    )
    .success();
    let before = tpl::config::Config::load(&project.path).unwrap();
    let old_tip = project.git(&["rev-parse", "refs/tpl/parent"]);
    source
        .write("README.md", "no rendered file changed\n")
        .commit_all("document source template");
    let new_sha = source.git(&["rev-parse", "HEAD"]);
    let status = tpl(&project, &["--json", "status"]).code(2).json();
    assert_eq!(status["composedMoved"], true);
    assert_eq!(status["availableComposed"][0]["commit"], new_sha);
    assert_eq!(
        status["renderedComposed"][0]["commit"],
        before.composed[0].commit
    );
    let updated = tpl(&project, &["--json", "update", "--defaults"])
        .success()
        .json();
    assert_eq!(updated["result"], "updated");
    assert_eq!(updated["changes"], serde_json::json!([]));
    assert_eq!(updated["composedChanged"], true);
    assert_ne!(project.git(&["rev-parse", "refs/tpl/parent"]), old_tip);
    assert_eq!(
        tpl::config::Config::load(&project.path).unwrap().composed[0].commit,
        new_sha
    );
    assert!(
        project
            .git(&["status", "--short"])
            .contains(".config/git.tpl.toml")
    );
}

#[test]
fn a_child_branch_that_changes_rendered_bytes_is_pending_and_updates_its_selected_modules() {
    let source = child("source");
    let integration = child("integration");
    let parent = parent(&source, &integration, "main", "main");
    let project = Repo::init("project");
    tpl(
        &project,
        &[
            "--json",
            "init",
            parent.path.to_str().unwrap(),
            "--answer",
            "sources=[\"jira\"]",
            "--defaults",
        ],
    )
    .success();
    source
        .write(
            "template/{{ module_path }}/module.txt.jinja",
            "changed: {{ module_name }}\n",
        )
        .commit_all("change module skeleton");
    let status = tpl(&project, &["--json", "status"]).code(2).json();
    assert_eq!(status["templateMoved"], false);
    assert_eq!(status["composedMoved"], true);
    let update = tpl(&project, &["--json", "update", "--defaults"])
        .success()
        .json();
    assert_eq!(update["changes"][0]["path"], "modules/jira/module.txt");
    assert_eq!(
        tpl::config::Config::load(&project.path).unwrap().composed[0].commit,
        source.git(&["rev-parse", "HEAD"])
    );
}

#[test]
fn deselecting_all_modules_removes_the_child_lock_without_fetching_its_old_repository() {
    let source = child("source");
    let integration = child("integration");
    let parent = parent(&source, &integration, "main", "main");
    let project = Repo::init("project");
    tpl(
        &project,
        &[
            "--json",
            "init",
            parent.path.to_str().unwrap(),
            "--answer",
            "sources=[\"jira\"]",
            "--defaults",
        ],
    )
    .success();
    let mut config = tpl::config::Config::load(&project.path).unwrap();
    config
        .answers
        .insert("sources".into(), tpl::template::Value::Array(Vec::new()));
    config.save(&project.path).unwrap();
    project.commit_all("deselect the source module");
    let moved = source.path.with_file_name("source-moved");
    std::fs::rename(&source.path, moved).unwrap();
    tpl(&project, &["--json", "update", "--defaults"]).success();
    assert!(
        tpl::config::Config::load(&project.path)
            .unwrap()
            .composed
            .is_empty()
    );
}

#[test]
fn status_keeps_the_last_child_commit_visible_when_its_repository_is_unavailable() {
    let source = child("source");
    let integration = child("integration");
    let parent = parent(&source, &integration, "main", "main");
    let project = Repo::init("project");
    tpl(
        &project,
        &[
            "--json",
            "init",
            parent.path.to_str().unwrap(),
            "--answer",
            "sources=[\"jira\"]",
            "--defaults",
        ],
    )
    .success();
    let recorded = tpl::config::Config::load(&project.path).unwrap().composed[0]
        .commit
        .clone();
    std::fs::rename(
        &source.path,
        source.path.with_file_name("source-unavailable"),
    )
    .unwrap();
    let status = tpl(&project, &["--json", "status"]).success().json();
    assert_eq!(status["renderedComposed"][0]["commit"], recorded);
    assert!(status["availableComposed"].is_null());
    assert_eq!(status["composedMoved"], false);
}

#[test]
fn backport_reproduces_the_original_children_after_their_branch_moves() {
    let source = child("source");
    let integration = child("integration");
    let parent = parent(&source, &integration, "main", "main");
    let project = Repo::init("project");
    tpl(
        &project,
        &[
            "--json",
            "init",
            parent.path.to_str().unwrap(),
            "--answer",
            "sources=[\"jira\"]",
            "--defaults",
        ],
    )
    .success();
    source
        .write(
            "template/{{ module_path }}/module.txt.jinja",
            "changed: {{ module_name }}\n",
        )
        .commit_all("move the source branch");
    project.write("README.md", "This is the parent\nExtra explanation.\n");
    let output = tpl(&project, &["--json", "backport", "README.md"])
        .success()
        .json();
    assert_eq!(output["result"], "patched", "{output}");
}

#[test]
fn backport_never_mistakes_a_child_generated_file_for_a_parent_template_file() {
    let source = child("source");
    let integration = child("integration");
    let parent = parent(&source, &integration, "main", "main");
    let project = Repo::init("project");
    tpl(
        &project,
        &[
            "--json",
            "init",
            parent.path.to_str().unwrap(),
            "--answer",
            "sources=[\"jira\"]",
            "--defaults",
        ],
    )
    .success();
    project.write("modules/jira/module.txt", "jira\nA local edit.\n");
    let output = tpl(&project, &["--json", "backport", "modules/jira/module.txt"]);
    assert_ne!(
        output.code,
        0,
        "child files must not be backported to the parent: {}",
        output.all()
    );
    assert_eq!(output.error_code(), "tpl::backport::unknown_path");
}

#[test]
fn changing_the_recorded_module_list_adds_a_child_on_update_without_another_attachment() {
    let source = child("source");
    let integration = child("integration");
    let source_ref = source.git(&["rev-parse", "HEAD"]);
    let integration_ref = integration.git(&["rev-parse", "HEAD"]);
    let parent = parent(&source, &integration, &source_ref, &integration_ref);
    let project = Repo::init("project");
    tpl(
        &project,
        &[
            "--json",
            "init",
            parent.path.to_str().unwrap(),
            "--answer",
            "sources=[\"jira\"]",
            "--defaults",
        ],
    )
    .success();
    assert!(project.exists("modules/jira/module.txt"));
    assert!(!project.exists("modules/confluence/module.txt"));
    let config = project.read(".config/git.tpl.toml");
    assert!(config.contains("jira"), "{config}");
    project.write(
        ".config/git.tpl.toml",
        &config.replace(
            "sources = [\"jira\"]",
            "sources = [\"jira\", \"confluence\"]",
        ),
    );
    project.commit_all("add a module to the answer set");
    tpl(&project, &["--json", "update", "--defaults"]).success();
    tpl(&project, &["--json", "merge"]).success();
    assert_eq!(project.read("modules/jira/module.txt"), "jira\n");
    assert_eq!(
        project.read("modules/confluence/module.txt"),
        "confluence\n"
    );
}

#[test]
fn a_child_changes_upstream_without_changing_a_pinned_parent_rendering() {
    let source = child("source");
    let integration = child("integration");
    let source_ref = source.git(&["rev-parse", "HEAD"]);
    let integration_ref = integration.git(&["rev-parse", "HEAD"]);
    let parent = parent(&source, &integration, &source_ref, &integration_ref);
    source.write(
        "template/{{ module_path }}/module.txt.jinja",
        "changed: {{ module_name }}\n",
    );
    source.commit_all("change child on main");
    let output = tempfile::tempdir().unwrap();
    let output = output.path().join("generated");
    tpl(
        &parent,
        &[
            "--json",
            "render",
            parent.path.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--answer",
            "sources=[\"jira\"]",
            "--defaults",
        ],
    )
    .success();
    assert_eq!(
        std::fs::read_to_string(output.join("modules/jira/module.txt")).unwrap(),
        "jira\n"
    );
}

#[test]
fn one_module_id_cannot_be_both_a_source_and_an_integration() {
    let source = child("source");
    let integration = child("integration");
    let source_ref = source.git(&["rev-parse", "HEAD"]);
    let integration_ref = integration.git(&["rev-parse", "HEAD"]);
    let parent = parent(&source, &integration, &source_ref, &integration_ref);
    let output = tempfile::tempdir().unwrap();
    let output = output.path().join("generated");
    let failure = tpl(
        &parent,
        &[
            "--json",
            "render",
            parent.path.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--answer",
            "sources=[\"duplicate\"]",
            "--answer",
            "integrations=[\"duplicate\"]",
            "--defaults",
        ],
    )
    .failure();
    assert_eq!(failure.error_code(), "tpl::compose::invalid");
    assert!(!output.exists());
}

#[test]
fn a_child_cannot_silently_replace_a_parent_file() {
    let source = child("source");
    let integration = child("integration");
    let source_ref = source.git(&["rev-parse", "HEAD"]);
    let integration_ref = integration.git(&["rev-parse", "HEAD"]);
    let parent = parent(&source, &integration, &source_ref, &integration_ref);
    parent.write("template/modules/jira/module.txt", "parent owns this\n");
    parent.commit_all("reserve child path");
    let output = tempfile::tempdir().unwrap();
    let output = output.path().join("generated");
    let failure = tpl(
        &parent,
        &[
            "--json",
            "render",
            parent.path.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--answer",
            "sources=[\"jira\"]",
            "--defaults",
        ],
    )
    .failure();
    assert_eq!(failure.error_code(), "tpl::compose::collision");
    assert!(!output.exists());
}

#[test]
fn a_remote_composed_child_is_refused_before_any_clone_without_consent() {
    let source = child("source");
    let integration = child("integration");
    let source_ref = source.git(&["rev-parse", "HEAD"]);
    let integration_ref = integration.git(&["rev-parse", "HEAD"]);
    let parent = parent(&source, &integration, &source_ref, &integration_ref);
    let manifest = parent.read("template.toml").replace(
        &source.path.display().to_string(),
        "https://example.invalid/module-template.git",
    );
    parent
        .write("template.toml", &manifest)
        .commit_all("request remote child");
    let output = tempfile::tempdir().unwrap();
    let output = output.path().join("generated");
    let failure = tpl(
        &parent,
        &[
            "--json",
            "render",
            parent.path.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--answer",
            "sources=[\"jira\"]",
            "--defaults",
        ],
    )
    .failure();
    assert_eq!(failure.error_code(), "tpl::compose::untrusted");
    assert!(!output.exists());
}

#[test]
fn a_parent_may_compose_modules_from_a_question_inherited_from_a_pinned_base() {
    let source = child("source");
    let integration = child("integration");
    let source_ref = source.git(&["rev-parse", "HEAD"]);
    let integration_ref = integration.git(&["rev-parse", "HEAD"]);
    let parent = parent(&source, &integration, &source_ref, &integration_ref);
    let base = Repo::init("base");
    base.write(
        "template.toml",
        "name = 'base'\n[questions.sources]\ntype = 'list'\ndefault = []\n",
    )
    .write("template/base.txt", "base\n")
    .commit_all("base questions");
    let base_ref = base.git(&["rev-parse", "HEAD"]);
    let manifest = parent
        .read("template.toml")
        .replace("[questions.sources]\ntype = 'list'\ndefault = []\n", "")
        .replace(
            "[questions.integrations]",
            &format!(
                "[extends]\nsource = '{}'\nrev = '{base_ref}'\n[questions.integrations]",
                base.path.display()
            ),
        );
    parent
        .write("template.toml", &manifest)
        .commit_all("compose from inherited list");
    let output = tempfile::tempdir().unwrap();
    let output = output.path().join("generated");
    tpl(
        &parent,
        &[
            "--json",
            "render",
            parent.path.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--answer",
            "sources=[\"jira\"]",
            "--defaults",
        ],
    )
    .success();
    assert_eq!(
        std::fs::read_to_string(output.join("modules/jira/module.txt")).unwrap(),
        "jira\n"
    );
}
