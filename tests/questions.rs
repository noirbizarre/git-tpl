//! Dynamic questions, computed values and data sources, end to end.

mod common;

use common::{World, tpl};

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
fn a_typo_is_reported_with_a_suggestion() {
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
fn an_undeclared_data_source_is_reported() {
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
        &[("x.txt", "x")],
    );

    world
        .init(&[])
        .failure()
        .says("things")
        .says("data/absent.toml");
}

#[test]
fn malformed_data_is_reported_with_the_source_that_failed() {
    let dir = tempfile::tempdir().unwrap();
    let template = common::Repo::init_in(dir.path(), "template");
    template.write(
        "template.toml",
        "name = \"bad\"\n[data.broken]\nsource = \"data/broken.toml\"\n[computed]\nx = \"{{ data.broken }}\"\n",
    );
    template.write("data/broken.toml", "not = = toml\n");
    template.write("template/x.txt", "x");
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
fn a_path_that_would_escape_the_tree_is_refused() {
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

#[test]
fn a_templated_path_segment_is_rendered() {
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

/// PLAN item 5, and the reason `default_from` is prompt-only. Under
/// `--defaults` nobody confirms anything, so the machine's `user.name` must not
/// become an answer.
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
