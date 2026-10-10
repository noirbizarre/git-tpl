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
        "name = 'parent'\n[questions.sources]\ntype = 'string'\ndefault = ''\n[questions.integrations]\ntype = 'string'\ndefault = ''\n[[compose]]\nsource = '{}'\nrev = '{source_ref}'\nfrom = 'sources'\n[[compose]]\nsource = '{}'\nrev = '{integration_ref}'\nfrom = 'integrations'\n",
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
            "sources=jira,confluence",
            "--answer",
            "integrations=codex",
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
fn a_child_branch_cannot_be_used_as_a_reproducible_revision() {
    let source = child("source");
    let integration = child("integration");
    let integration_ref = integration.git(&["rev-parse", "HEAD"]);
    let parent = parent(&source, &integration, "main", &integration_ref);
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
            "sources=jira",
            "--defaults",
        ],
    )
    .failure();
    assert_eq!(failure.error_code(), "tpl::compose::unpinned");
    assert!(!output.exists());
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
            "sources=jira",
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
        &config.replace("jira", "jira,confluence"),
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
            "sources=jira",
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
            "sources=duplicate",
            "--answer",
            "integrations=duplicate",
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
            "sources=jira",
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
            "sources=jira",
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
        "name = 'base'\n[questions.sources]\ntype = 'string'\ndefault = ''\n",
    )
    .write("template/base.txt", "base\n")
    .commit_all("base questions");
    let base_ref = base.git(&["rev-parse", "HEAD"]);
    let manifest = parent
        .read("template.toml")
        .replace("[questions.sources]\ntype = 'string'\ndefault = ''\n", "")
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
            "sources=jira",
            "--defaults",
        ],
    )
    .success();
    assert_eq!(
        std::fs::read_to_string(output.join("modules/jira/module.txt")).unwrap(),
        "jira\n"
    );
}
