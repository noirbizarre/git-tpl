//! Actual expression execution, rather than graph reachability, controls acquisition.

mod common;

use common::{World, tpl};

const DECLARATIONS: &str = r#"
name = "lazy"
[data.absent]
source = "missing.toml"
[data.remote]
source = "https://example.invalid/unused.json"
[computed]
unused = "{{ data.absent.value }}"
"#;

#[test]
fn unused_and_untaken_sources_do_not_load_even_offline() {
    for body in [
        "literal",
        "{{ false and data.remote.value }}",
        "{{ true or unused }}",
        "{{ 'local' if true else data.absent.value }}",
        "{% if false %}{{ unused }}{% endif %}local",
    ] {
        let world = World::with_template(DECLARATIONS, &[("result.jinja", body)]);
        world.init(&["--offline", "--trust"]).success();
        let message = world
            .project
            .git(&["show", "-s", "--format=%B", &world.ref_name()]);
        assert!(!message.contains("Data-Source:"), "{message}");
    }
}

#[test]
fn skipped_questions_do_not_demand_choices_or_transitive_defaults() {
    let manifest = format!(
        "{DECLARATIONS}\n[questions.choice]\ntype = 'choice'\nwhen = '{{{{ false }}}}'\nchoices_from = 'data.absent.items'\n[questions.text]\ntype = 'string'\nwhen = '{{{{ false }}}}'\ndefault = '{{{{ unused }}}}'\n"
    );
    let world = World::with_template(&manifest, &[("result", "local")]);
    world.init(&["--offline"]).success();
}

#[test]
fn supplied_answers_do_not_demand_their_unused_defaults() {
    let manifest = format!(
        "{DECLARATIONS}\n[questions.text]\ntype = 'string'\ndefault = '{{{{ unused }}}}'\n"
    );
    let world = World::with_template(&manifest, &[("result.jinja", "{{ text }}")]);
    world.init(&["--answer", "text=supplied"]).success();
    assert_eq!(world.project.read("result"), "supplied");
}

#[test]
fn suppressed_paths_skip_later_segments_and_bodies() {
    let world = World::with_template(
        DECLARATIONS,
        &[
            (
                "{% if false %}skip{% endif %}/{{ unused }}/body.jinja",
                "{{ data.remote.value }}",
            ),
            ("kept", "local"),
        ],
    );
    world.init(&["--offline"]).success();
    assert_eq!(world.project.read("kept"), "local");
}

#[test]
fn demanded_data_errors_keep_their_original_diagnostics() {
    for (body, code) in [
        ("{{ unused }}", "tpl::data::load"),
        ("{{ data.remote.value }}", "tpl::data::untrusted"),
    ] {
        let world = World::with_template(DECLARATIONS, &[("result.jinja", body)]);
        assert_eq!(world.init(&["--json"]).failure().error_code(), code);
    }
}

#[test]
fn explicit_context_evaluation_can_demand_data_not_used_by_files() {
    let world = World::with_template(
        "name = 'inspect'\n[data.settings]\nsource = 'settings.toml'\n",
        &[("result", "local")],
    );
    world.template.repo.write("settings.toml", "value = 42\n");
    world.template.repo.commit_all("data for inspection");
    let output = tpl(
        &world.project,
        &[
            "--json",
            "context",
            &world.template.source(),
            "--defaults",
            "--eval",
            "{{ data.settings.value }}",
        ],
    )
    .success()
    .json();
    assert_eq!(output["value"], 42);
}

#[test]
fn defined_default_and_whole_table_operations_do_not_hide_demand() {
    for body in [
        "{{ data.settings is defined }}",
        "{{ data.settings.value | default('wrong') }}",
        "{{ data }}",
        "{% for key, value in data | items %}{{ value.value }}{% endfor %}",
    ] {
        let world = World::with_template(
            "name = 'inspect'\n[data.settings]\nsource = 'settings.toml'\n",
            &[("result.jinja", body)],
        );
        world.template.repo.write("settings.toml", "value = 42\n");
        world.template.repo.commit_all("data");
        world.init(&[]).success();
        let message = world
            .project
            .git(&["show", "-s", "--format=%B", &world.ref_name()]);
        assert_eq!(message.matches("Data-Source:").count(), 1, "{message}");
        assert!(!world.project.read("result").contains("wrong"));
    }
}

#[test]
fn the_first_demand_prevents_loading_a_speculative_branch() {
    let manifest = format!("{DECLARATIONS}\nflag = '{{{{ true }}}}'\n");
    let world = World::with_template(
        &manifest,
        &[(
            "result.jinja",
            "{% if flag %}local{% else %}{{ data.remote.value }}{% endif %}",
        )],
    );
    world.init(&["--offline", "--trust"]).success();
    assert_eq!(world.project.read("result"), "local");
}

#[test]
fn dynamic_source_dependencies_and_file_only_computed_values_are_resolved() {
    let world = World::with_template(
        r#"
name = "dynamic"
[data.index]
source = "index.toml"
[data.settings]
source = "{{ data.index.path }}"
[computed]
answer = "{{ data.settings.value }}"
"#,
        &[("result.jinja", "{{ answer }} {{ answer }}")],
    );
    world
        .template
        .repo
        .write("index.toml", "path = 'settings.toml'\n");
    world.template.repo.write("settings.toml", "value = 42\n");
    world.template.repo.commit_all("data dependency");
    world.init(&[]).success();
    assert_eq!(world.project.read("result"), "42 42");
    let message = world
        .project
        .git(&["show", "-s", "--format=%B", &world.ref_name()]);
    assert_eq!(message.matches("Data-Source:").count(), 2);
    let before = world.project.rev_parse(&world.ref_name());
    tpl(&world.project, &["update", "--defaults"]).success();
    assert_eq!(world.project.rev_parse(&world.ref_name()), before);
}

#[test]
fn notes_and_new_migration_messages_can_be_the_only_data_consumers() {
    let world = World::with_template(
        "name = 'metadata'\nnote = '{{ data.settings.value }}'\n[data.settings]\nsource = 'settings.toml'\n",
        &[("result", "local")],
    );
    world
        .template
        .repo
        .write("settings.toml", "value = 'hello'\n");
    world.template.repo.commit_all("note data");
    world.init(&[]).success().says("hello");
    world.add_migration(
        "000-message.toml",
        "message = '{{ data.settings.value }}'\n",
    );
    tpl(&world.project, &["update", "--defaults"])
        .success()
        .says("hello");
}

#[test]
fn an_injected_gated_default_intentionally_demands_its_data() {
    let manifest = format!(
        "{DECLARATIONS}\n[questions.text]\ntype = 'string'\nwhen = '{{{{ false }}}}'\ndefault_when_skipped = true\ndefault = '{{{{ unused }}}}'\n"
    );
    let world = World::with_template(&manifest, &[("result", "local")]);
    assert_eq!(
        world.init(&["--json"]).failure().error_code(),
        "tpl::data::load"
    );
}

#[test]
fn standalone_inspection_does_not_acquire_unused_git_local_or_malformed_sources() {
    let world = World::with_template(
        r#"
name = "isolated"
[data.repo]
source = "https://example.invalid/data.git"
ref = "main"
path = "data.toml"
[data.local]
source = "./missing.toml"
[data.broken]
source = "broken.toml"
"#,
        &[("result", "local")],
    );
    world.template.repo.write("broken.toml", "not = = toml");
    world.template.repo.commit_all("unused sources");
    let output = tpl(
        &world.project,
        &[
            "--offline",
            "--json",
            "context",
            &world.template.source(),
            "--defaults",
        ],
    )
    .success()
    .json();
    assert_eq!(output["data"], serde_json::json!({}));
}

#[test]
fn untaken_partial_branches_do_not_acquire_data() {
    let world = World::with_template(
        DECLARATIONS,
        &[("result.jinja", "{% include 'partial.jinja' %}")],
    );
    world.template.repo.write(
        "partial.jinja",
        "{% if false %}{{ unused }}{% else %}local{% endif %}",
    );
    world.template.repo.commit_all("conditional partial");
    world.init(&["--offline"]).success();
    assert_eq!(world.project.read("result"), "local");
}
