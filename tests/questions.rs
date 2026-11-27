//! Dynamic questions, computed values and data sources, end to end.

mod common;

use common::{Template, World, tpl, tpl_outside};

#[test]
fn selection_bounds_are_exposed_and_invalid_schemas_are_rejected() {
    let world = World::with_template(
        "name = 'bounds'\n[questions.pick]\ntype = 'multi_choice'\nchoices = ['a', 'b']\nmin_choices = 1\nmax_choices = 2\n[questions.other]\ntype = 'boolean'\ndefault = false\n",
        &[("out.jinja", "{{ pick }}")],
    );
    let schema = Scratch::new().ask(&world.template.source());
    assert_eq!(question(&schema, "pick")["minChoices"], 1);
    assert_eq!(question(&schema, "pick")["maxChoices"], 2);
    assert!(question(&schema, "other")["minChoices"].is_null());
    assert!(question(&schema, "other")["maxChoices"].is_null());

    for body in [
        "type = 'string'\nmax_choices = 1",
        "type = 'list'\nmin_choices = 1",
        "type = 'choice'\nchoices = ['a']\nmax_choices = 1",
        "type = 'multi_choice'\nchoices = ['a']\nmin_choices = 2\nmax_choices = 1",
        "type = 'multi_choice'\nchoices = ['a']\nmin_choices = 2",
        "type = 'multi_choice'\nchoices = ['a', 'a']\nmin_choices = 2",
        "type = 'multi_choice'\nchoices = ['a']\nmax_choices = -1",
        "type = 'multi_choice'\nchoices = ['a']\nmax_choices = 1.5",
        "type = 'multi_choice'\nchoices = ['a']\nmin_choices = '1'",
    ] {
        assert!(
            tpl::template::Manifest::parse(
                &format!("name = 'bounds'\n[questions.pick]\n{body}\n"),
                "template.toml"
            )
            .is_err(),
            "{body}"
        );
    }
}

#[test]
fn question_schema_exposes_the_list_item_constraints_enforced_by_evaluation() {
    let world = World::with_template(
        "name = 'lists'\n[questions.modules]\ntype = 'list'\ndefault = []\nitem_pattern = '^[a-z]+$'\nitem_message = 'lowercase module names only'\n",
        &[("modules.jinja", "{{ modules | join(',') }}")],
    );
    let schema = Scratch::new().ask(&world.template.source());
    let modules = question(&schema, "modules");
    assert_eq!(modules["itemPattern"], "^[a-z]+$");
    assert_eq!(modules["itemMessage"], "lowercase module names only");
    world
        .init(&["--answer", "modules=[\"INVALID\"]"])
        .failure()
        .says("lowercase module names only");
}

#[test]
fn static_choice_introspection_uses_formats_checksums_and_the_render_choice_reader() {
    for (file, format, body) in [
        (
            "choices.json",
            "",
            r#"[{"value":"a","label":"Alpha","help":"First"},"b"]"#,
        ),
        (
            "choices.yaml",
            "",
            "- value: a\n  label: Alpha\n  help: First\n- b\n",
        ),
        (
            "choices.data",
            "format = 'toml'",
            "items = [{value = 'a', label = 'Alpha', help = 'First'}, 'b']",
        ),
    ] {
        let reference = if file.ends_with("data") {
            "data.options.items"
        } else {
            "data.options"
        };
        let world = World::with_template(
            &format!(
                "name = 'choices'\n[data.options]\nsource = '{file}'\n{format}\n[questions.pick]\ntype = 'choice'\nchoices_from = '{reference}'\n"
            ),
            &[("keep", "")],
        );
        world
            .template
            .repo
            .write(file, body)
            .commit_all("static choices");
        let json = Scratch::new().ask(&world.template.source());
        let pick = question(&json, "pick");
        assert_eq!(pick["choicesResolved"], serde_json::json!(["a", "b"]));
        assert_eq!(
            pick["choicesResolvedDetails"][0],
            serde_json::json!({"value":"a","label":"Alpha","help":"First"})
        );
        assert_eq!(pick["choicesResolvedDetails"][1]["label"], "b");
        let manifest = world.template.repo.read("template.toml");
        world
            .template
            .repo
            .write(
                "template.toml",
                &manifest.replace(
                    &format!("source = '{file}'"),
                    &format!("source = '{file}'\nsha256 = '{}'", "0".repeat(64)),
                ),
            )
            .commit_all("incorrect checksum");
        assert_eq!(
            tpl(
                &world.project,
                &["--json", "questions", &world.template.source()]
            )
            .failure()
            .error_code(),
            "tpl::data::checksum"
        );
    }
    let world = World::with_template(
        "name = 'bad'\n[data.options]\nsource = 'choices.json'\n[questions.pick]\ntype = 'choice'\nchoices_from = 'data.options'\n",
        &[("keep", "")],
    );
    world
        .template
        .repo
        .write("choices.json", r#"["a",42]"#)
        .commit_all("invalid choice member");
    assert_eq!(
        tpl(
            &world.project,
            &["--json", "questions", &world.template.source()]
        )
        .failure()
        .error_code(),
        "tpl::eval::bad_choices"
    );
}

#[test]
fn inherited_choices_read_the_declaring_layer_until_overridden() {
    let base = common::Repo::init("base");
    base.write("template.toml", "name = 'base'\n[data.options]\nsource = 'choices.json'\n[questions.pick]\ntype = 'choice'\nchoices_from = 'data.options'\n")
        .write("template/keep", "")
        .write("choices.json", "[\"base\"]").commit_all("base choices");
    let leaf = common::Repo::init("leaf");
    let manifest = format!(
        "name = 'leaf'\n[extends]\nsource = '{}'\nrev = '{}'\n",
        base.path.display(),
        base.git(&["rev-parse", "HEAD"])
    );
    leaf.write("template.toml", &manifest)
        .write("choices.json", "[\"leaf\"]")
        .write("template/keep", "")
        .commit_all("inherit choices");
    assert_eq!(
        question(&Scratch::new().ask(leaf.path.to_str().unwrap()), "pick")["choicesResolved"],
        serde_json::json!(["base"])
    );
    leaf.write(
        "template.toml",
        &format!("{manifest}\n[data.options]\nsource = 'choices.json'\n"),
    )
    .commit_all("override choices");
    assert_eq!(
        question(&Scratch::new().ask(leaf.path.to_str().unwrap()), "pick")["choicesResolved"],
        serde_json::json!(["leaf"])
    );
}

/// A manifest exercising conditionals, dynamic defaults and computed values.
const DYNAMIC: &str = r#"
name = "dynamic"

[questions.project_name]
type = "string"
default = "My Project"

[questions.project_type]
type = "choice"
choices = ["library", "application"]
default = "library"

[questions.cli]
type = "boolean"
when = "{{ project_type == 'application' }}"
default = true

[questions.package_name]
type = "string"
default = "{{ project_name | lower | replace(' ', '-') }}"

[computed]
module_name = "{{ package_name | replace('-', '_') }}"
"#;

const CARGO: &str = r#"[package]
name = "{{ package_name }}"
module = "{{ module_name }}"
{% if cli is defined and cli %}
[[bin]]
name = "{{ package_name }}"
{% endif %}
"#;

fn dynamic_world() -> World {
    World::with_template(DYNAMIC, &[("Cargo.toml.jinja", CARGO)])
}

fn list_world() -> World {
    World::with_template(
        "name = 'free-list'\n[questions.modules]\ntype = 'list'\ndefault = []\nitem_pattern = '^[a-z][a-z-]*$'\nitem_message = 'use lowercase module IDs'\n",
        &[(
            "modules.txt.jinja",
            "{% for name in modules %}{{ name }}\n{% endfor %}",
        )],
    )
}

#[test]
fn a_free_form_list_answer_is_recorded_as_an_ordered_array_not_comma_separated_text() {
    let world = list_world();
    world
        .init(&["--answer", "modules=[\"jira\",\"confluence\"]"])
        .success();
    assert_eq!(world.project.read("modules.txt"), "jira\nconfluence\n");
    let config = world.project.read(".config/git.tpl.toml");
    let parsed = tpl::config::Config::parse(&config, "config").unwrap();
    assert_eq!(
        parsed.answers["modules"],
        tpl::template::Value::from(vec!["jira", "confluence"])
    );
}

#[test]
fn an_empty_list_is_a_valid_recorded_answer() {
    let world = list_world();
    world.init(&[]).success();
    assert!(world.project.read("modules.txt").is_empty());
    assert!(
        world
            .project
            .read(".config/git.tpl.toml")
            .contains("modules = []")
    );
}

#[test]
fn a_list_refuses_non_string_and_malformed_or_empty_entries_before_rendering() {
    for (answer, code) in [
        ("modules=[\"jira\",123]", "tpl::eval::invalid_list"),
        ("modules=[\"jira\",\"\"]", "tpl::eval::invalid_list"),
        ("modules=[\"jira\",\" Bad\"]", "tpl::eval::invalid_list"),
        ("modules=jira,confluence", "tpl::eval::invalid_list"),
    ] {
        let world = list_world();
        let outcome = world.init(&["--json", "--answer", answer]).failure();
        assert_eq!(outcome.error_code(), code, "{answer}");
        assert!(!world.project.exists("modules.txt"));
    }
}

#[test]
fn a_dynamic_default_is_computed_from_an_earlier_answer() {
    let world = dynamic_world();

    world
        .init(&["--answer", "project_name=My Great Lib"])
        .success();

    assert!(
        world
            .project
            .read(".config/git.tpl.toml")
            .contains("package_name = \"my-great-lib\"")
    );
}

#[test]
fn a_computed_value_is_available_to_the_template() {
    let world = dynamic_world();

    world.init(&["--answer", "project_name=My Lib"]).success();

    assert!(
        world
            .project
            .read("Cargo.toml")
            .contains("module = \"my_lib\"")
    );
}

/// A skipped question must be *absent*, not null — that is what lets a template
/// tell "not applicable" from "declined".
#[test]
fn a_question_whose_condition_is_false_is_not_asked_or_recorded() {
    let world = dynamic_world();

    world.init(&["--answer", "project_type=library"]).success();

    let config = world.project.read(".config/git.tpl.toml");
    assert!(
        !config.contains("cli"),
        "a skipped question must not be recorded:\n{config}"
    );
    assert!(
        !world.project.read("Cargo.toml").contains("[[bin]]"),
        "`cli is defined` must be false for a library"
    );
}

#[test]
fn a_question_whose_condition_is_true_is_asked_and_recorded() {
    let world = dynamic_world();

    world
        .init(&["--answer", "project_type=application"])
        .success();

    assert!(
        world
            .project
            .read(".config/git.tpl.toml")
            .contains("cli = true")
    );
    assert!(world.project.read("Cargo.toml").contains("[[bin]]"));
}

#[test]
fn choices_can_come_from_a_data_source() {
    let world = World::new();

    world.init(&["--answer", "license=Apache-2.0"]).success();

    assert!(
        world
            .project
            .read("README.md")
            .contains("Apache License 2.0")
    );
}

/// Types survive the journey from a data file into a template.
#[test]
fn data_keeps_its_types() {
    let dir = tempfile::tempdir().unwrap();
    let template = common::Repo::init_in(dir.path(), "template");
    template.write(
        "template.toml",
        r#"
name = "typed"
[data.ci]
source = "data/ci.toml"
"#,
    );
    template.write(
        "data/ci.toml",
        "timeout = 30\nstrict = true\nversions = [\"1.88\", \"stable\"]\n",
    );
    template.write(
        "template/out.txt.jinja",
        "{% if data.ci.strict %}timeout={{ data.ci.timeout + 1 }} first={{ data.ci.versions[0] }}{% endif %}\n",
    );
    template.commit_all("feat: initial");

    let project = common::Repo::init_in(dir.path(), "project");
    project.write("x", "x");
    project.commit_all("chore: initial");

    tpl(
        &project,
        &["init", &template.path.to_string_lossy(), "--defaults"],
    )
    .success();

    assert_eq!(
        project.read("out.txt"),
        "timeout=31 first=1.88\n",
        "an integer must stay an integer and a bool a bool"
    );
}

#[test]
fn json_data_works_the_same_as_toml() {
    let dir = tempfile::tempdir().unwrap();
    let template = common::Repo::init_in(dir.path(), "template");
    template.write(
        "template.toml",
        "name = \"j\"\n[data.reg]\nsource = \"data/reg.json\"\n",
    );
    template.write("data/reg.json", r#"{"items": ["a", "b"], "count": 2}"#);
    template.write(
        "template/out.txt.jinja",
        "{{ data.reg.count }}:{{ data.reg.items | join(',') }}\n",
    );
    template.commit_all("feat: initial");

    let project = common::Repo::init_in(dir.path(), "project");
    project.write("x", "x");
    project.commit_all("chore: initial");

    tpl(
        &project,
        &["init", &template.path.to_string_lossy(), "--defaults"],
    )
    .success();

    assert_eq!(project.read("out.txt"), "2:a,b\n");
}

/// Template data is read from the template's Git tree at the resolved
/// revision, so it is pinned by the template revision like everything else.
#[test]
fn template_data_is_read_at_the_resolved_revision() {
    let world = World::new();
    world.template.repo.git(&["tag", "v1.0.0"]);

    world.template.repo.write(
        "data/licenses.toml",
        "ids = [\"MIT\", \"Apache-2.0\", \"BSD-3-Clause\"]\n\n[names]\nMIT = \"Changed Name\"\n\"Apache-2.0\" = \"A\"\n\"BSD-3-Clause\" = \"B\"\n",
    );
    world
        .template
        .repo
        .commit_all("chore: change the licence names");

    world.init(&["--ref", "v1.0.0"]).success();

    assert!(
        world.project.read("README.md").contains("MIT License"),
        "v1.0.0's data must be used, not the tip's"
    );
}

// --- errors, caught before anything is asked --------------------------------

#[test]
fn a_cycle_is_reported_before_any_prompt() {
    let world = World::with_template(
        r#"
name = "cyclic"
[computed]
a = "{{ b }}"
b = "{{ a }}"
"#,
        &[("x.txt", "x")],
    );

    world
        .init(&[])
        .failure()
        .says("cyclic dependency")
        .says("computed.a");

    assert!(!world.project.exists(".config/git.tpl.toml"));
}

#[test]
fn init_refuses_a_typo_in_an_expression_and_suggests_the_name_meant() {
    let world = World::with_template(
        r#"
name = "typo"
[questions.project_name]
type = "string"
default = "x"
[computed]
package_name = "{{ projct_name | lower }}"
"#,
        &[("x.txt", "x")],
    );

    world
        .init(&[])
        .failure()
        .says("projct_name")
        .says("did you mean `project_name`?");
}

#[test]
fn init_refuses_a_choices_from_naming_a_data_source_the_manifest_never_declared() {
    let world = World::with_template(
        r#"
name = "missing-data"
[questions.license]
type = "choice"
choices_from = "data.licenses.ids"
"#,
        &[("x.txt", "x")],
    );

    world.init(&[]).failure().says("data.licenses");
}

#[test]
fn a_data_file_that_does_not_exist_is_reported_with_its_path() {
    let world = World::with_template(
        r#"
name = "absent-data"
[data.things]
source = "data/absent.toml"
[computed]
x = "{{ data.things }}"
"#,
        &[("x.txt.jinja", "{{ x }}")],
    );

    world
        .init(&[])
        .failure()
        .says("things")
        .says("data/absent.toml");
}

#[test]
fn init_names_the_data_source_whose_file_would_not_parse() {
    let dir = tempfile::tempdir().unwrap();
    let template = common::Repo::init_in(dir.path(), "template");
    template.write(
        "template.toml",
        "name = \"bad\"\n[data.broken]\nsource = \"data/broken.toml\"\n[computed]\nx = \"{{ data.broken }}\"\n",
    );
    template.write("data/broken.toml", "not = = toml\n");
    template.write("template/x.txt.jinja", "{{ x }}");
    template.commit_all("feat: initial");

    let project = common::Repo::init_in(dir.path(), "project");
    project.write("x", "x");
    project.commit_all("chore: initial");

    tpl(
        &project,
        &["init", &template.path.to_string_lossy(), "--defaults"],
    )
    .failure()
    .says("broken");
}

#[test]
fn a_question_and_a_computed_value_may_not_share_a_name() {
    let world = World::with_template(
        r#"
name = "collision"
[questions.package_name]
type = "string"
default = "x"
[computed]
package_name = "{{ 'y' }}"
"#,
        &[("x.txt", "x")],
    );

    world.init(&[]).failure().says("package_name");
}

/// The reason literals are accepted: a constant shared by several rendered
/// files, used as the number it is. Writing `"{{ 100 }}"` worked but read as a
/// workaround, and the type error it replaced said nothing useful.
#[test]
fn a_literal_computed_value_reaches_the_template_as_its_toml_type() {
    let world = World::with_template(
        r#"
name = "literal"
[computed]
line_length = 100
"#,
        &[(
            "ruff.toml.jinja",
            "line-length = {{ line_length }}\nhalf = {{ line_length // 2 }}\n",
        )],
    );

    world.init(&[]).success();

    let rendered = world.project.read("ruff.toml");
    assert!(
        rendered.contains("line-length = 100"),
        "a literal integer must not be stringified:\n{rendered}"
    );
    assert!(
        rendered.contains("half = 50"),
        "arithmetic proves it arrived as a number:\n{rendered}"
    );
}

#[test]
fn a_question_with_no_default_and_no_answer_is_reported_under_defaults() {
    let world = World::with_template(
        r#"
name = "needs-answer"
[questions.project_name]
type = "string"
"#,
        &[("x.txt", "x")],
    );

    world
        .init(&[])
        .failure()
        .says("project_name")
        .says("--answer");
}

/// A supplied answer is parsed into the question's declared type; a value that
/// cannot be must not be silently coerced.
#[test]
fn a_supplied_answer_of_the_wrong_type_is_refused() {
    let world = World::with_template(
        r#"
name = "typed"
[questions.port]
type = "integer"
default = 8080
"#,
        &[("x.txt", "port={{ port }}")],
    );

    world.init(&["--answer", "port=nope"]).failure();
}

#[test]
fn a_malformed_manifest_is_reported() {
    let world = World::with_template("name = = broken\n", &[("x.txt", "x")]);

    world.init(&[]).failure();
}

/// A template repository is untrusted input, and `..` in a rendered path is a
/// request to write outside the tree.
#[test]
fn init_refuses_a_path_that_would_escape_the_tree() {
    let world = World::with_template(
        r#"
name = "escape"
[questions.evil]
type = "string"
default = ".."
"#,
        &[("{{ evil }}/x.txt", "x")],
    );

    world.init(&[]).failure().says("escapes the tree");
}

/// A segment rendering empty is how a template makes a whole subtree
/// conditional.
#[test]
fn a_conditional_directory_is_omitted_when_its_condition_is_false() {
    let world = World::with_template(
        r#"
name = "conditional"
[questions.ci]
type = "boolean"
default = true
"#,
        &[
            (
                "{% if ci %}.github{% endif %}/workflows/ci.yml",
                "name: CI\n",
            ),
            ("README.md", "readme\n"),
        ],
    );

    world.init(&["--answer", "ci=false"]).success();
    assert!(!world.project.exists(".github/workflows/ci.yml"));
    assert!(world.project.exists("README.md"));
}

#[test]
fn a_conditional_directory_is_rendered_when_its_condition_holds() {
    let world = World::with_template(
        r#"
name = "conditional"
[questions.ci]
type = "boolean"
default = true
"#,
        &[(
            "{% if ci %}.github{% endif %}/workflows/ci.yml",
            "name: CI\n",
        )],
    );

    world.init(&["--answer", "ci=true"]).success();
    assert!(world.project.exists(".github/workflows/ci.yml"));
}

/// A directory segment rendering to `.` is transparent, not pruned: its
/// children still render, one level up from where the directory would have
/// been.
#[test]
fn a_transparent_path_segment_flattens_its_directory() {
    let world = World::with_template(
        r#"
name = "transparent"
[questions.use_mid]
type = "boolean"
default = false
"#,
        &[(
            "{% if use_mid %}mid{% else %}.{% endif %}/leaf.txt",
            "hello\n",
        )],
    );

    world.init(&["--answer", "use_mid=false"]).success();
    assert!(!world.project.exists("mid"));
    assert_eq!(world.project.read("leaf.txt"), "hello\n");
}

#[test]
fn the_same_transparent_segment_keeps_the_directory_when_its_condition_holds() {
    let world = World::with_template(
        r#"
name = "transparent"
[questions.use_mid]
type = "boolean"
default = false
"#,
        &[(
            "{% if use_mid %}mid{% else %}.{% endif %}/leaf.txt",
            "hello\n",
        )],
    );

    world.init(&["--answer", "use_mid=true"]).success();
    assert!(!world.project.exists("leaf.txt"));
    assert_eq!(world.project.read("mid/leaf.txt"), "hello\n");
}

/// `.` has no "above" to promote to when it is the file's own name, so it
/// still escapes the tree there.
#[test]
fn init_refuses_a_transparent_marker_on_a_files_own_name() {
    let world = World::with_template(
        r#"
name = "transparent"
[questions.named]
type = "boolean"
default = false
"#,
        &[("dir/{% if named %}name{% else %}.{% endif %}", "x")],
    );

    world
        .init(&["--answer", "named=false"])
        .failure()
        .says("escapes the tree");
}

/// The issue's own motivating example: one computed value builds a
/// variable-depth path by fanning out across `/`, rather than duplicating
/// the subtree once per layout or chaining a `.`-transparent segment per
/// level.
#[test]
fn a_computed_path_segment_fans_out_into_nested_directories() {
    let world = World::with_template(
        r#"
name = "fanout"
[questions.use_src]
type = "boolean"
default = false
[computed]
package_path = "{{ 'src/' if use_src else '' }}"
"#,
        &[("{{ package_path }}pkg/__init__.py", "# pkg\n")],
    );

    world.init(&["--answer", "use_src=false"]).success();
    assert!(world.project.exists("pkg/__init__.py"));
    assert!(!world.project.exists("src"));
}

#[test]
fn the_same_computed_path_segment_nests_deeper_when_the_toggle_is_on() {
    let world = World::with_template(
        r#"
name = "fanout"
[questions.use_src]
type = "boolean"
default = false
[computed]
package_path = "{{ 'src/' if use_src else '' }}"
"#,
        &[("{{ package_path }}pkg/__init__.py", "# pkg\n")],
    );

    world.init(&["--answer", "use_src=true"]).success();
    assert!(world.project.exists("src/pkg/__init__.py"));
}

#[test]
fn a_templated_path_segment_reaches_the_project_rendered() {
    let world = World::with_template(
        r#"
name = "paths"
[questions.project_name]
type = "string"
default = "demo"
[computed]
package_name = "{{ project_name | lower }}"
"#,
        &[(
            "src/{{ package_name }}/mod.rs.jinja",
            "//! {{ project_name }}\n",
        )],
    );

    world.init(&[]).success();
    assert_eq!(world.project.read("src/demo/mod.rs"), "//! demo\n");
}

/// A label is presentation, so editing one must not change the rendered tree.
/// If it did, every project using the template would get a commit — and a merge
/// to perform — because somebody improved the wording of a prompt.
#[test]
fn renaming_a_choice_label_produces_no_commit() {
    let world = World::with_template(
        r#"
name = "labelled"
[questions.license]
type = "choice"
choices = [
  { value = "MIT", label = "MIT License", help = "Permissive" },
  { value = "Apache-2.0", label = "Apache License 2.0" },
]
default = "MIT"
"#,
        &[("LICENSE.jinja", "{{ license }}\n")],
    );

    world.init(&[]).success();
    let before = world.project.rev_parse(&world.ref_name());
    assert_eq!(world.project.read("LICENSE"), "MIT\n");

    world.template.repo.write(
        "template.toml",
        r#"
name = "labelled"
[questions.license]
type = "choice"
choices = [
  { value = "MIT", label = "The MIT Licence", help = "Short and permissive" },
  { value = "Apache-2.0", label = "Apache 2.0" },
]
default = "MIT"
"#,
    );
    world
        .template
        .repo
        .commit_all("docs: reword the licence labels");

    tpl(&world.project, &["update", "--defaults"]).success();

    assert_eq!(
        world.project.rev_parse(&world.ref_name()),
        before,
        "a label is not part of the rendered tree, so the ref must not move"
    );
}

/// Choices are filtered with `[computed]`. When a template narrows a filter, a
/// previously recorded answer can fall outside it — and that must be reported,
/// not silently dropped, because dropping it would change the rendered tree
/// and commit without anyone asking.
#[test]
fn an_answer_a_narrowed_filter_no_longer_offers_is_reported() {
    let manifest = |extra: &str| {
        format!(
            r#"
name = "filtered"

[questions.tier]
type = "choice"
choices = ["free", "pro"]
default = "pro"

[computed]
regions = "{{{{ ['eu', 'us'{extra}] }}}}"

[questions.region]
type = "choice"
choices_from = "regions"
default = "eu"
"#
        )
    };

    let world = World::with_template(
        &manifest(", 'ap'"),
        &[("region.txt.jinja", "{{ region }}\n")],
    );

    world.init(&["--answer", "region=ap"]).success();
    assert_eq!(world.project.read("region.txt"), "ap\n");

    // The template drops `ap`, which this project is using.
    world.template.repo.write("template.toml", &manifest(""));
    world
        .template
        .repo
        .commit_all("feat: withdraw the ap region");

    tpl(&world.project, &["update", "--defaults"])
        .failure()
        .says("region")
        .says("eu, us")
        .says(".config/git.tpl.toml");

    assert_eq!(
        world.project.read("region.txt"),
        "ap\n",
        "a failed update must leave the worktree alone"
    );
}

/// The combination that looked as though it needed a `multi_choice_from` key:
/// `type` and the source of the choices are independent.
#[test]
fn a_multi_choice_draws_labelled_choices_from_a_data_source() {
    let world = World::with_template(
        r#"
name = "features"

[data.catalogue]
source = "data/features.toml"

[questions.features]
type = "multi_choice"
choices_from = "data.catalogue.all"
default = ["serde"]
"#,
        &[("features.txt.jinja", "{{ features | join(',') }}\n")],
    );

    world.template.repo.write(
        "data/features.toml",
        r#"
[[all]]
value = "serde"
label = "Serialisation"
help = "Derive Serialize and Deserialize"

[[all]]
value = "async"
label = "Async runtime"
"#,
    );
    world
        .template
        .repo
        .commit_all("feat: add the feature catalogue");

    world.init(&["--answer", "features=serde,async"]).success();

    assert_eq!(world.project.read("features.txt"), "serde,async\n");
    assert!(
        world
            .project
            .read(".config/git.tpl.toml")
            .contains(r#""serde""#),
        "the value is recorded, never the label"
    );
}

/// A filter that narrows to nothing means "this does not apply", so the
/// question is absent rather than empty — `is defined` still tells the two
/// apart, exactly as it does for a false `when`.
#[test]
fn a_question_with_no_remaining_choices_is_skipped() {
    let world = World::with_template(
        r#"
name = "narrowing"

[questions.kind]
type = "choice"
choices = ["library", "application"]
default = "library"

[computed]
servers = "{{ ['nginx', 'caddy'] if kind == 'application' else [] }}"

[questions.server]
type = "choice"
choices_from = "servers"
default = "nginx"
"#,
        &[(
            "out.txt.jinja",
            "{% if server is defined %}server={{ server }}{% else %}no server{% endif %}\n",
        )],
    );

    world.init(&[]).success();
    assert_eq!(world.project.read("out.txt"), "no server\n");
    assert!(
        !world
            .project
            .read(".config/git.tpl.toml")
            .contains("server"),
        "a skipped question records no answer"
    );
}

// --- git-seeded prompt defaults ---------------------------------------------

/// `default_from` seeds a prompt and nothing else.
const SEEDED: &str = r#"
name = "seeded"

[questions.author]
type = "string"
default = "anonymous"
default_from = "git:user.name"
"#;

fn seeded_world(author: &str) -> World {
    let world = World::with_template(SEEDED, &[("AUTHORS.jinja", "{{ author }}\n")]);
    // Per-repository, never global — the harness never touches the developer's
    // own configuration.
    world.project.git(&["config", "user.name", author]);
    world
}

/// The reason `default_from` is prompt-only. Under `--defaults` nobody
/// confirms anything, so the machine's `user.name` must not become an answer.
#[test]
fn a_git_seeded_default_is_not_used_when_questions_are_not_asked() {
    let world = seeded_world("Ada Lovelace");

    let output = world.init(&[]).success();

    assert_eq!(world.project.read("AUTHORS"), "anonymous\n");
    assert!(
        world
            .project
            .read(".config/git.tpl.toml")
            .contains("author = \"anonymous\""),
        "the recorded answer must be the template's own default"
    );
    output.silent_about("Ada Lovelace");
}

/// The claim in one assertion: the same template, answered the same way, is the
/// same tree on two machines whose Git identities differ.
#[test]
fn two_machines_render_the_same_tree_from_a_git_seeded_default() {
    let one = seeded_world("Ada Lovelace");
    let two = seeded_world("Grace Hopper");

    one.init(&[]).success();
    two.init(&[]).success();

    assert_eq!(
        one.project
            .git(&["rev-parse", &format!("{}^{{tree}}", one.ref_name())]),
        two.project
            .git(&["rev-parse", &format!("{}^{{tree}}", two.ref_name())]),
    );
}

/// Git configuration values are strings, so the manifest refuses the
/// combinations that could only fail on one person's machine.
#[test]
fn default_from_on_a_non_string_question_is_reported_before_any_prompt() {
    let world = World::with_template(
        r#"
name = "bad"

[questions.ci]
type = "boolean"
default_from = "git:tpl.ci"
"#,
        &[("out.txt.jinja", "x\n")],
    );

    world
        .init(&[])
        .failure()
        .says("only applies to `string` questions");
}

// --- seeds derived from the repository ---------------------------------------

const USER_SEEDED: &str = r#"
name = "os-user"
[questions.username]
type = "string"
default = "declared"
default_from = "{{ user.name | default('developer') }}"
"#;

#[test]
fn an_os_login_seed_requires_human_confirmation() {
    let world = World::with_template(USER_SEEDED, &[("USER.jinja", "{{ username }}\n")]);
    world.init(&[]).success();
    assert_eq!(world.project.read("USER"), "declared\n");
    assert!(
        world
            .project
            .read(".config/git.tpl.toml")
            .contains("username = \"declared\"")
    );
}

#[test]
fn an_interactive_os_login_seed_is_independent_of_git_identity_and_personal_defaults_win() {
    struct AcceptSeed(tpl::Value);
    impl tpl::eval::Prompter for AcceptSeed {
        fn ask(
            &mut self,
            _: &str,
            _: &tpl::template::Question,
            _: Option<&tpl::Value>,
            seed: Option<&tpl::Value>,
            _: Option<&[tpl::template::Choice]>,
        ) -> Result<tpl::Value, tpl::eval::EvalError> {
            assert_eq!(seed, Some(&self.0));
            Ok(self.0.clone())
        }
    }
    let login = whoami::username_os()
        .ok()
        .and_then(|name| name.into_string().ok())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "developer".into());
    for personal in [false, true] {
        let world = World::with_template(USER_SEEDED, &[("USER.jinja", "{{ username }}\n")]);
        // A visibly different Git identity catches accidentally aliasing git.user.name.
        world
            .project
            .git(&["config", "user.name", &format!("Git identity for {login}")]);
        let repo = tpl::git::libgit2::LibGit2::open(&world.project.path).unwrap();
        let mut user = tpl::userconfig::UserConfig::default();
        let expected = tpl::Value::String(if personal {
            "preferred-login".into()
        } else {
            login.clone()
        });
        if personal {
            user.defaults.insert("username".into(), expected.clone());
            // A winning preference must bypass acquisition, not just replace its result.
            world
                .project
                .git(&["config", "tpl.autoPush", "not-a-boolean"]);
        }
        let result = tpl::ops::render_files(
            tpl::ops::Target {
                source: &world.template.source(),
                reference: None,
                root: None,
                dirty: false,
            },
            Some((&repo, &world.project.path)),
            tpl::answers::SuppliedAnswers::new(),
            &user,
            &tpl::git::cache::Access::new(None, false),
            tpl::ops::Answering::Interactive(&mut AcceptSeed(expected.clone())),
            tpl::ops::Trust::always(),
        )
        .unwrap();
        assert_eq!(result.context.answers().get("username"), Some(&expected));
        let tpl::Value::String(expected) = expected else {
            unreachable!("the accepted login is text")
        };
        let output = result
            .files
            .iter()
            .find(|file| file.path == "USER")
            .unwrap();
        assert_eq!(output.content, format!("{expected}\n").as_bytes());
    }
}

#[test]
fn an_os_login_seed_does_not_expose_the_account_to_defaults_or_rendering() {
    let ordinary_default = World::with_template(
        &USER_SEEDED.replace("default = \"declared\"", "default = \"{{ user.name }}\""),
        &[("USER.jinja", "{{ username }}\n")],
    );
    ordinary_default.init(&[]).failure().says("user");
    let rendered = World::with_template(USER_SEEDED, &[("USER.jinja", "{{ user.name }}\n")]);
    rendered.init(&[]).failure().says("user");
}

const DATETIME_SEEDED: &str = r#"
name = "datetime"
[questions.copyright_year]
type = "string"
default = "2000"
default_from = "{{ now.year }}"
"#;

#[test]
fn a_datetime_seed_is_ignored_without_human_confirmation() {
    let world = World::with_template(DATETIME_SEEDED, &[("YEAR.jinja", "{{ copyright_year }}\n")]);
    world.init(&[]).success();
    assert_eq!(world.project.read("YEAR"), "2000\n");
    let required = World::with_template(
        &DATETIME_SEEDED.replace("default = \"2000\"\n", ""),
        &[("YEAR.jinja", "{{ copyright_year }}\n")],
    );
    required.init(&[]).failure().says("copyright_year");
}

#[test]
fn an_interactive_datetime_seed_is_offered_as_text_and_personal_defaults_win() {
    struct AcceptSeed {
        expected: Option<tpl::Value>,
    }
    impl tpl::eval::Prompter for AcceptSeed {
        fn ask(
            &mut self,
            _: &str,
            _: &tpl::template::Question,
            _: Option<&tpl::Value>,
            seed: Option<&tpl::Value>,
            _: Option<&[tpl::template::Choice]>,
        ) -> Result<tpl::Value, tpl::eval::EvalError> {
            let seed = seed.expect("the interactive question must offer a seed");
            if let Some(expected) = &self.expected {
                assert_eq!(seed, expected);
            } else {
                let tpl::Value::String(year) = seed else {
                    panic!("a numeric seed must become prompt text")
                };
                assert!(year.parse::<i32>().is_ok());
            }
            Ok(seed.clone())
        }
    }
    for personal in [false, true] {
        let world =
            World::with_template(DATETIME_SEEDED, &[("YEAR.jinja", "{{ copyright_year }}\n")]);
        let repo = tpl::git::libgit2::LibGit2::open(&world.project.path).unwrap();
        let mut user = tpl::userconfig::UserConfig::default();
        let expected = personal.then(|| tpl::Value::String("1999".into()));
        if let Some(value) = &expected {
            user.defaults.insert("copyright_year".into(), value.clone());
            // A winning personal default must skip acquisition, not merely
            // overwrite the clock seed after reading repository preferences.
            world
                .project
                .git(&["config", "tpl.autoPush", "not-a-boolean"]);
        }
        let result = tpl::ops::render_files(
            tpl::ops::Target {
                source: &world.template.source(),
                reference: None,
                root: None,
                dirty: false,
            },
            Some((&repo, &world.project.path)),
            tpl::answers::SuppliedAnswers::new(),
            &user,
            &tpl::git::cache::Access::new(None, false),
            tpl::ops::Answering::Interactive(&mut AcceptSeed { expected }),
            tpl::ops::Trust::always(),
        )
        .unwrap();
        assert!(matches!(
            result.context.answers().get("copyright_year"),
            Some(tpl::Value::String(_))
        ));
    }
}

#[test]
fn a_datetime_seed_does_not_make_the_clock_available_to_ordinary_defaults() {
    let world = World::with_template(
        &DATETIME_SEEDED.replace("default = \"2000\"", "default = \"{{ now.year }}\""),
        &[("YEAR.jinja", "{{ copyright_year }}\n")],
    );
    world.init(&[]).failure().says("now");
}

#[test]
fn unused_runtime_seeds_do_not_fail_interactive_or_defaults_only_rendering() {
    struct NeverAsk;
    impl tpl::eval::Prompter for NeverAsk {
        fn ask(
            &mut self,
            _: &str,
            _: &tpl::template::Question,
            _: Option<&tpl::Value>,
            _: Option<&tpl::Value>,
            _: Option<&[tpl::template::Choice]>,
        ) -> Result<tpl::Value, tpl::eval::EvalError> {
            panic!("a skipped or supplied question must not prompt")
        }
    }
    for (when, supplied, defaults_only) in [
        ("false", false, false),
        ("true", true, false),
        ("true", false, true),
    ] {
        let world = World::with_template(
            &format!(
                "name = 'lazy'\n[questions.author]\ntype = 'string'\nwhen = '{{{{ {when} }}}}'\ndefault = 'declared'\ndefault_from = '{{{{ git.user.name() }}}}'\n"
            ),
            &[("out.jinja", "{{ author | default('skipped') }}")],
        );
        // Invalid acquisition preferences prove we did not merely discard an
        // evaluated seed. No seed-related configuration may be read at all.
        world
            .project
            .git(&["config", "tpl.autoPush", "not-a-boolean"]);
        let repo = tpl::git::libgit2::LibGit2::open(&world.project.path).unwrap();
        let mut answers = tpl::answers::SuppliedAnswers::new();
        if supplied {
            answers.insert(
                "author".into(),
                tpl::answers::SuppliedAnswer::Cli("supplied".into()),
            );
        }
        let mut prompter = NeverAsk;
        let result = tpl::ops::render_files(
            tpl::ops::Target {
                source: &world.template.source(),
                reference: None,
                root: None,
                dirty: false,
            },
            Some((&repo, &world.project.path)),
            answers,
            &Default::default(),
            &tpl::git::cache::Access::new(None, false),
            if defaults_only {
                tpl::ops::Answering::defaults()
            } else {
                tpl::ops::Answering::Interactive(&mut prompter)
            },
            tpl::ops::Trust::always(),
        )
        .unwrap();
        assert_eq!(
            result.context.answers().get("author").cloned(),
            if supplied {
                Some(tpl::Value::String("supplied".into()))
            } else if defaults_only {
                Some(tpl::Value::String("declared".into()))
            } else {
                None
            }
        );
    }
}

/// The case the feature exists for: a template asks for a slug, and the project
/// already knows one — from where it is pushed, or failing that from what the
/// directory is called.
const DERIVED: &str = r#"
name = "derived"

[questions.slug]
type = "string"
default = "placeholder"
default_from = "{{ remote.name | default(dir.name) | slugify }}"
"#;

fn derived_world(remote: Option<&str>) -> World {
    let world = World::with_template(DERIVED, &[("SLUG.jinja", "{{ slug }}\n")]);
    if let Some(url) = remote {
        world.project.git(&["remote", "add", "origin", url]);
    }
    world
}

/// The same guard `git:user.name` has, for the wider set of sources. Under
/// `--defaults` nobody confirms anything, so nothing read from the machine may
/// become an answer — otherwise two developers commit two different trees.
#[test]
fn a_derived_seed_is_not_used_when_questions_are_not_asked() {
    let world = derived_world(Some("git@github.com:me/guessed-name.git"));

    let output = world.init(&[]).success();

    assert_eq!(world.project.read("SLUG"), "placeholder\n");
    assert!(
        world
            .project
            .read(".config/git.tpl.toml")
            .contains("slug = \"placeholder\""),
        "the recorded answer must be the template's own default"
    );
    output.silent_about("guessed-name");
}

/// The claim in one assertion: two projects that would be *seeded* differently
/// — one from a remote, one with no remote at all — still render the same tree,
/// because neither seed was used.
#[test]
fn two_machines_render_the_same_tree_from_a_derived_default() {
    let one = derived_world(Some("git@github.com:me/one.git"));
    let two = derived_world(None);

    one.init(&[]).success();
    two.init(&[]).success();

    assert_eq!(
        one.project
            .git(&["rev-parse", &format!("{}^{{tree}}", one.ref_name())]),
        two.project
            .git(&["rev-parse", &format!("{}^{{tree}}", two.ref_name())]),
    );
}

/// A syntax error in a seed expression belongs to the template author, and they
/// must meet it on their own first render rather than on a user's machine.
#[test]
fn a_broken_default_from_expression_is_reported_before_any_prompt() {
    let world = World::with_template(
        r#"
name = "bad"

[questions.slug]
type = "string"
default_from = "{{ remote.name | }}"
"#,
        &[("out.txt.jinja", "x\n")],
    );

    world.init(&[]).failure().says("not a valid expression");
}

/// The seed context is not the render context, and a template author reaching
/// for an answer inside a `default_from` has misunderstood which is which. A
/// chainable environment would render it to nothing and say so never.
#[test]
fn a_default_from_expression_referencing_a_question_is_refused() {
    let world = World::with_template(
        r#"
name = "bad"

[questions.project_name]
type = "string"
default = "demo"

[questions.slug]
type = "string"
default_from = "{{ project_name | slugify }}"
"#,
        &[("out.txt.jinja", "x\n")],
    );

    world.init(&[]).failure().says("seed namespace");
}

/// The manifest field that stops a bad distribution name reaching the first
/// build. A pattern, never an expression: an arbitrary validator would be code
/// running on a template's behalf, and invariant 5 says no.
const PATTERNED: &str = r#"
name = "patterned"

[questions.slug]
type = "string"
default = "thing"
pattern = "^[a-z][a-z0-9-]*$"
message = "must be lowercase and start with a letter"
"#;

fn patterned_world() -> World {
    World::with_template(PATTERNED, &[("slug.txt.jinja", "{{ slug }}\n")])
}

#[test]
fn an_answer_matching_the_pattern_is_accepted() {
    let world = patterned_world();

    world.init(&["--answer", "slug=my-thing"]).success();
    assert_eq!(world.project.read("slug.txt"), "my-thing\n");
}

/// The check has to cover the supplied path, not only the prompt: `--answer`
/// is exactly how a bad value would otherwise reach a commit.
#[test]
fn an_answer_the_pattern_refuses_is_reported() {
    let world = patterned_world();

    world
        .init(&["--answer", "slug=My Thing"])
        .failure()
        .says("slug")
        .says("must be lowercase and start with a letter");
}

/// Without a `message` there is still something to say — the pattern itself.
#[test]
fn a_pattern_without_a_message_reports_the_pattern() {
    let world = World::with_template(
        r#"
name = "bare"

[questions.slug]
type = "string"
default = "thing"
pattern = "^[a-z]+$"
"#,
        &[("slug.txt.jinja", "{{ slug }}\n")],
    );

    world
        .init(&["--answer", "slug=NOPE"])
        .failure()
        .says("^[a-z]+$");
}

/// A question that does not apply has no answer at all, so there is nothing to
/// match — a pattern must not resurrect it.
#[test]
fn a_skipped_question_is_never_checked_against_its_pattern() {
    let world = World::with_template(
        r#"
name = "conditional"

[questions.publish]
type = "boolean"
default = false

[questions.slug]
type = "string"
when = "{{ publish }}"
default = "NOT A SLUG"
pattern = "^[a-z]+$"
"#,
        &[("out.txt.jinja", "{{ publish }}\n")],
    );

    world.init(&[]).success();
}

/// The same reasoning as a withdrawn choice: a template that narrows what it
/// accepts must say so, rather than silently rendering a tree from a value it
/// would no longer allow.
#[test]
fn an_answer_a_narrowed_pattern_no_longer_accepts_is_reported() {
    let manifest = |pattern: &str| {
        format!(
            r#"
name = "narrowing"

[questions.slug]
type = "string"
default = "thing"
pattern = "{pattern}"
"#
        )
    };

    let world = World::with_template(
        &manifest("^[A-Za-z-]+$"),
        &[("slug.txt.jinja", "{{ slug }}\n")],
    );

    world.init(&["--answer", "slug=My-Thing"]).success();
    assert_eq!(world.project.read("slug.txt"), "My-Thing\n");

    // The template tightens the pattern, and this project's recorded answer no
    // longer matches it.
    world
        .template
        .repo
        .write("template.toml", &manifest("^[a-z-]+$"));
    world.template.repo.commit_all("feat!: slugs are lowercase");

    tpl(&world.project, &["update", "--defaults"])
        .failure()
        .says("slug")
        .says(".config/git.tpl.toml");

    assert_eq!(
        world.project.read("slug.txt"),
        "My-Thing\n",
        "a failed update must leave the worktree alone"
    );
}

/// A pattern matches text. On a boolean it could only ever always pass or
/// always fail, and the author would render to find out which.
#[test]
fn a_pattern_on_a_non_string_question_is_reported_before_any_prompt() {
    let world = World::with_template(
        r#"
name = "bad"

[questions.ci]
type = "boolean"
pattern = "^y"
"#,
        &[("out.txt.jinja", "x\n")],
    );

    world
        .init(&[])
        .failure()
        .says("`pattern` only applies to `string` questions");
}

/// Compiled when the manifest is read, so the template author is the one who
/// finds out — not a user, six questions into a questionnaire.
#[test]
fn an_uncompilable_pattern_is_reported_before_any_prompt() {
    let world = World::with_template(
        r#"
name = "bad"

[questions.slug]
type = "string"
pattern = "^[a-z"
"#,
        &[("out.txt.jinja", "x\n")],
    );

    world
        .init(&[])
        .failure()
        .says("not a valid regular expression");
}

/// Almost always a `pattern` that was renamed or removed, leaving behind a
/// message nothing would ever show.
#[test]
fn a_message_without_a_pattern_is_reported_before_any_prompt() {
    let world = World::with_template(
        r#"
name = "bad"

[questions.slug]
type = "string"
message = "lowercase only"
"#,
        &[("out.txt.jinja", "x\n")],
    );

    world.init(&[]).failure().says("`message` has no `pattern`");
}

// ---------------------------------------------------------------------------
// `git tpl questions` — the schema, without asking anything.
//
// Everything above drives the questionnaire through `init`. These drive the
// declaration of it, which is what a caller that cannot answer a prompt needs.
// ---------------------------------------------------------------------------

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

    fn ask(&self, source: &str) -> serde_json::Value {
        tpl_outside(
            self.dir.path(),
            self.config.path(),
            &["--json", "questions", source],
        )
        .success()
        .json()
    }

    /// The same schema, as the listing a person reads.
    fn ask_text(&self, source: &str) -> common::Output {
        tpl_outside(self.dir.path(), self.config.path(), &["questions", source])
    }
}

fn names(json: &serde_json::Value) -> Vec<String> {
    json["questions"]
        .as_array()
        .expect("questions")
        .iter()
        .map(|q| q["name"].as_str().expect("name").to_string())
        .collect()
}

fn question<'a>(json: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    json["questions"]
        .as_array()
        .expect("questions")
        .iter()
        .find(|q| q["name"] == name)
        .unwrap_or_else(|| panic!("no question {name}"))
}

#[test]
fn the_schema_needs_no_repository_and_no_prompt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = Template::standard(dir.path());
    let scratch = Scratch::new();

    let json = scratch.ask(&template.source());
    assert_eq!(json["ok"], true);
    assert_eq!(json["template"]["name"], "rust-library");
    assert!(names(&json).contains(&"project_name".to_string()));
    assert!(!scratch.dir.path().join(".git").exists());
}

/// Declaration order is not answer order. When a `when` or a `default`
/// references an earlier answer, this is the order a caller has to answer in —
/// and getting it wrong means asking for a value that does not exist yet.
#[test]
fn questions_are_listed_in_resolution_order_not_declaration_order() {
    let world = World::with_template(
        r#"
name = "ordered"

# Declared first, but depends on `base` — so it must be reported second.
[questions.derived]
type = "string"
default = "{{ base }}-suffix"

[questions.base]
type = "string"
default = "root"
"#,
        &[("file.txt.jinja", "{{ derived }}\n")],
    );
    let scratch = Scratch::new();

    let json = scratch.ask(&world.template.source());
    assert_eq!(names(&json), ["base", "derived"]);
    assert_eq!(question(&json, "base")["order"], 0);
    assert_eq!(question(&json, "derived")["order"], 1);
}

/// A default may be an expression. A caller that treated `"{{ crate }}"` as a
/// literal would write it verbatim into the answers file.
#[test]
fn an_expression_default_is_flagged_as_one() {
    let world = World::with_template(
        r#"
name = "derived"

[questions.crate_name]
type = "string"
default = "demo"

[questions.bin_name]
type = "string"
default = "{{ crate_name }}"
"#,
        &[("file.txt.jinja", "{{ bin_name }}\n")],
    );
    let scratch = Scratch::new();

    let json = scratch.ask(&world.template.source());
    assert_eq!(question(&json, "crate_name")["defaultIsExpression"], false);
    assert_eq!(question(&json, "bin_name")["defaultIsExpression"], true);
    assert_eq!(question(&json, "bin_name")["default"], "{{ crate_name }}");
}

/// `choices_from` names a path into a data file. Resolving it here saves the
/// caller fetching and parsing the file itself.
#[test]
fn choices_from_a_template_data_file_are_resolved() {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = Template::standard(dir.path());
    let scratch = Scratch::new();

    let json = scratch.ask(&template.source());
    let license = question(&json, "license");
    assert_eq!(license["choicesFrom"], "data.licenses.ids");

    let resolved = license["choicesResolved"]
        .as_array()
        .expect("choicesResolved");
    assert!(
        resolved.iter().any(|value| value == "MIT"),
        "expected MIT among {resolved:?}"
    );
}

#[test]
fn a_conditional_question_reports_its_condition() {
    let world = World::with_template(
        r#"
name = "gated"

[questions.docs]
type = "boolean"
default = true

[questions.accent]
type = "choice"
when = "docs"
choices = ["indigo", "teal"]
default = "indigo"
"#,
        &[("file.txt.jinja", "x\n")],
    );
    let scratch = Scratch::new();

    let json = scratch.ask(&world.template.source());
    let accent = question(&json, "accent");
    assert_eq!(accent["when"], "docs");
    assert_eq!(accent["type"], "choice");
    assert_eq!(accent["choices"].as_array().expect("choices").len(), 2);
}

/// The opt-in ADR-025 adds (issue #117): the schema mirrors the manifest key
/// verbatim, the same way `when` and `pattern` already do.
#[test]
fn default_when_skipped_is_reported_in_the_schema() {
    let world = World::with_template(
        r#"
name = "gated"

[questions.docs]
type = "boolean"
default = true

[questions.docs_accent]
type = "string"
when = "{{ docs }}"
default = "blue"
default_when_skipped = true
"#,
        &[("file.txt.jinja", "x\n")],
    );
    let scratch = Scratch::new();

    let json = scratch.ask(&world.template.source());
    assert_eq!(question(&json, "docs_accent")["defaultWhenSkipped"], true);
    assert_eq!(question(&json, "docs")["defaultWhenSkipped"], false);
}

#[test]
fn a_pattern_and_its_message_are_reported() {
    let world = World::with_template(
        r#"
name = "validated"

[questions.slug]
type = "string"
pattern = "^[a-z]+$"
message = "lowercase letters only"
default = "demo"
"#,
        &[("file.txt.jinja", "{{ slug }}\n")],
    );
    let scratch = Scratch::new();

    let json = scratch.ask(&world.template.source());
    assert_eq!(question(&json, "slug")["pattern"], "^[a-z]+$");
    assert_eq!(question(&json, "slug")["message"], "lowercase letters only");
}

#[test]
fn computed_values_and_data_sources_are_listed_separately() {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = Template::standard(dir.path());
    let scratch = Scratch::new();

    let json = scratch.ask(&template.source());
    let computed = json["computed"].as_array().expect("computed");
    assert!(computed.iter().any(|value| value == "package_name"));

    let data = json["data"].as_array().expect("data");
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["name"], "licenses");
    assert_eq!(data[0]["source"], "data/licenses.toml");
}

// ---------------------------------------------------------------------------
// The text listing.
//
// The JSON schema above is what a program consumes; this is what an author
// runs to remember what their own template asks. Until these existed the
// whole branch had never run.
// ---------------------------------------------------------------------------

#[test]
fn the_text_listing_names_the_template_and_its_questions_in_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = Template::standard(dir.path());
    let scratch = Scratch::new();

    let output = scratch.ask_text(&template.source()).success();

    output
        .says("rust-library")
        // The description, which is optional and present here.
        .says("A small Rust library")
        .says("Questions, in the order they are asked")
        // Name and type, because the type is what decides how to answer it.
        .says("project_name (string)")
        .says("license (choice)");
}

/// The listing must name the kind the manifest declares, not the type an
/// answer parses as: `choice`, never `a string`. They differ for exactly the
/// two kinds an author is most likely to look up.
#[test]
fn the_text_listing_names_the_declared_kind_not_the_value_type() {
    let world = World::with_template(
        r#"
name = "kinds"

[questions.license]
type = "choice"
choices = ["MIT", "Apache-2.0"]

[questions.features]
type = "multi_choice"
choices = ["cli", "lib"]

[questions.msrv]
type = "boolean"
"#,
        &[("file.txt.jinja", "x\n")],
    );
    let scratch = Scratch::new();

    let output = scratch.ask_text(&world.template.source()).success();

    output
        .says("license (choice)")
        .says("features (multi_choice)")
        .says("msrv (boolean)")
        .silent_about("an array");
}

/// A `when` says the question is not always asked, which is the difference
/// between an answer file that works and one that silently ignores a key.
#[test]
fn the_text_listing_says_when_a_question_is_conditional() {
    let world = World::with_template(
        r#"
name = "gated"

[questions.docs]
type = "boolean"
default = true

[questions.accent]
type = "string"
when = "docs"
default = "indigo"
"#,
        &[("file.txt.jinja", "x\n")],
    );
    let scratch = Scratch::new();

    scratch
        .ask_text(&world.template.source())
        .success()
        .says("accent (string) when docs");
}

/// A template may legitimately ask nothing. Saying so beats printing a heading
/// followed by silence, which reads as a failure to load the manifest.
#[test]
fn a_template_that_asks_nothing_says_so() {
    let world = World::with_template(
        r#"name = "questionless""#,
        &[("file.txt.jinja", "{{ template.name }}\n")],
    );
    let scratch = Scratch::new();

    scratch
        .ask_text(&world.template.source())
        .success()
        .says("(none)");
}

// ---------------------------------------------------------------------------
// When `choices_from` is *not* resolved statically.
//
// Resolving it means reading the file at schema time, with no answers and no
// project. Three shapes make that impossible, and in each the honest answer is
// to report `choicesFrom` and omit `choicesResolved` — a guess would be a
// wrong answer dressed as a real one.
// ---------------------------------------------------------------------------

/// Fetching over the network to describe a schema would make `questions` an
/// operation that can fail because a server is down.
#[test]
fn choices_from_a_remote_source_are_not_resolved() {
    let world = World::with_template(
        r#"
name = "remote-choices"

[data.licenses]
source = "https://example.invalid/licenses.toml"

[questions.license]
type = "choice"
choices_from = "data.licenses.ids"
default = "MIT"
"#,
        &[("file.txt.jinja", "{{ license }}\n")],
    );
    let scratch = Scratch::new();

    let json = scratch.ask(&world.template.source());
    let license = question(&json, "license");
    assert_eq!(license["choicesFrom"], "data.licenses.ids");
    assert!(
        license.get("choicesResolved").is_none(),
        "a remote source was resolved anyway: {license}"
    );
}

/// Resolving a git source means a clone, and `questions` only reads a manifest.
/// The source here looks like an ordinary template path, so nothing but `ref`
/// and `path` says otherwise — which is exactly the case a guard on `source`
/// alone would miss.
#[test]
fn choices_from_a_git_source_are_not_resolved() {
    let world = World::with_template(
        r#"
name = "git-choices"

[data.licenses]
source = "git@example.invalid:acme/data"
ref = "v1"
path = "licenses.toml"

[questions.license]
type = "choice"
choices_from = "data.licenses.ids"
default = "MIT"
"#,
        &[("file.txt.jinja", "{{ license }}\n")],
    );
    let scratch = Scratch::new();

    let json = scratch.ask(&world.template.source());
    let license = question(&json, "license");
    assert!(
        license.get("choicesResolved").is_none(),
        "a git source was resolved, which would mean a clone: {license}"
    );
}

/// A leading `./` is a file in the *project*, and there is no project here.
#[test]
fn choices_from_a_project_local_source_are_not_resolved() {
    let world = World::with_template(
        r#"
name = "local-choices"

[data.licenses]
source = "./licenses.toml"

[questions.license]
type = "choice"
choices_from = "data.licenses.ids"
default = "MIT"
"#,
        &[("file.txt.jinja", "{{ license }}\n")],
    );
    let scratch = Scratch::new();

    let json = scratch.ask(&world.template.source());
    let license = question(&json, "license");
    assert!(
        license.get("choicesResolved").is_none(),
        "a project-local source was resolved without a project: {license}"
    );
}

/// An interpolated source depends on an answer, which is precisely what has
/// not been collected at the time the schema is described.
#[test]
fn choices_from_an_interpolated_source_are_not_resolved() {
    let world = World::with_template(
        r#"
name = "interpolated-choices"

[questions.flavour]
type = "string"
default = "mit"

[data.licenses]
source = "data/{{ flavour }}.toml"

[questions.license]
type = "choice"
choices_from = "data.licenses.ids"
default = "MIT"
"#,
        &[("file.txt.jinja", "{{ license }}\n")],
    );
    let scratch = Scratch::new();

    let json = scratch.ask(&world.template.source());
    let license = question(&json, "license");
    assert!(
        license.get("choicesResolved").is_none(),
        "a source depending on an answer was resolved before the answer: {license}"
    );
}

/// An explicit non-`template` kind is a source read some other way.
#[test]
fn choices_from_a_non_template_kind_are_not_resolved() {
    let world = World::with_template(
        r#"
name = "kinded-choices"

[data.licenses]
source = "licenses.toml"
kind = "project"

[questions.license]
type = "choice"
choices_from = "data.licenses.ids"
default = "MIT"
"#,
        &[("file.txt.jinja", "{{ license }}\n")],
    );
    let scratch = Scratch::new();

    let json = scratch.ask(&world.template.source());
    let license = question(&json, "license");
    assert!(
        license.get("choicesResolved").is_none(),
        "a project-kind source was read out of the template: {license}"
    );
}
