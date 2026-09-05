//! Shared macros — `{% import %}` and `{% include %}` against the template tree.
//!
//! The rule these tests exist to protect: **a partial is a `.jinja` file
//! outside the render root.** Outside the root is what keeps a macro definition
//! from being rendered into the project, and it is why no manifest key and no
//! skip rule were needed to support this.
//!
//! See `docs/adr/012-template-loader.md`.

mod common;

use common::{World, tpl};

const MANIFEST: &str = r#"
name = "demo"

[questions.project_name]
type = "string"
prompt = "Project name"
default = "demo"
"#;

/// The motivating case: one macro, used by two files.
#[test]
fn a_template_sharing_a_macro_renders_it_into_the_project() {
    let world = World::with_shared_template(
        MANIFEST,
        &[
            (
                "README.md.jinja",
                "{% import 'macros.jinja' as m %}{{ m.heading(project_name) }}\n",
            ),
            (
                "CONTRIBUTING.md.jinja",
                "{% import 'macros.jinja' as m %}{{ m.heading('Contributing') }}\n",
            ),
        ],
        &[(
            "macros.jinja",
            "{% macro heading(text) %}# {{ text }}{% endmacro %}",
        )],
    );

    world.init(&[]).success();

    assert_eq!(world.project.read("README.md"), "# demo\n");
    assert_eq!(world.project.read("CONTRIBUTING.md"), "# Contributing\n");
}

/// The property that makes the outside-the-root rule work: the tree walk only
/// ever sees the root subtree, so a partial cannot reach the project.
#[test]
fn a_partial_is_not_rendered_into_the_project() {
    let world = World::with_shared_template(
        MANIFEST,
        &[(
            "README.md.jinja",
            "{% import 'macros.jinja' as m %}{{ m.heading(project_name) }}\n",
        )],
        &[(
            "macros.jinja",
            "{% macro heading(text) %}# {{ text }}{% endmacro %}",
        )],
    );

    world.init(&[]).success();

    let rendered = world.project.tree_paths(&world.ref_name());
    assert_eq!(rendered, ["README.md"]);
}

/// A partial may live in a directory of its own, named by its full path from
/// the repository root — not from the render root.
#[test]
fn a_partial_in_a_subdirectory_is_imported_by_its_full_path() {
    let world = World::with_shared_template(
        MANIFEST,
        &[(
            "Cargo.toml.jinja",
            "{% import 'macros/rust.jinja' as m %}{{ m.package(project_name) }}\n",
        )],
        &[(
            "macros/rust.jinja",
            "{% macro package(name) %}[package]\nname = \"{{ name }}\"{% endmacro %}",
        )],
    );

    world.init(&[]).success();

    assert_eq!(
        world.project.read("Cargo.toml"),
        "[package]\nname = \"demo\"\n"
    );
    assert_eq!(world.project.tree_paths(&world.ref_name()), ["Cargo.toml"]);
}

/// A partial participates in the rendered tree's identity. If it did not,
/// editing a macro would be a template change that produced no commit — which
/// is precisely the failure the ref model exists to make impossible.
#[test]
fn changing_a_macro_advances_the_template_ref() {
    let world = World::with_shared_template(
        MANIFEST,
        &[(
            "README.md.jinja",
            "{% import 'macros.jinja' as m %}{{ m.heading(project_name) }}\n",
        )],
        &[(
            "macros.jinja",
            "{% macro heading(text) %}# {{ text }}{% endmacro %}",
        )],
    );
    world.init(&[]).success();

    let before = world.project.git(&["rev-parse", &world.ref_name()]);

    world.template.repo.write(
        "macros.jinja",
        "{% macro heading(text) %}## {{ text }}{% endmacro %}",
    );
    world.template.repo.commit_all("style: demote the heading");

    tpl(&world.project, &["update", "--defaults"]).success();

    let after = world.project.git(&["rev-parse", &world.ref_name()]);
    assert_ne!(before, after, "editing a macro produced no commit");
    assert_eq!(
        world
            .project
            .git(&["show", &format!("{}:README.md", world.ref_name())]),
        "## demo"
    );
}

/// Invariant 2, end to end. An untouched macro must produce no commit, or every
/// `update` would create noise the user has to merge.
#[test]
fn an_unchanged_macro_produces_no_commit() {
    let world = World::with_shared_template(
        MANIFEST,
        &[(
            "README.md.jinja",
            "{% import 'macros.jinja' as m %}{{ m.heading(project_name) }}\n",
        )],
        &[(
            "macros.jinja",
            "{% macro heading(text) %}# {{ text }}{% endmacro %}",
        )],
    );
    world.init(&[]).success();

    let before = world.project.git(&["rev-parse", &world.ref_name()]);
    tpl(&world.project, &["update", "--defaults"]).success();
    let after = world.project.git(&["rev-parse", &world.ref_name()]);

    assert_eq!(before, after);
}

/// A miss is nearly always a typo. Naming the template that failed is not
/// enough — the diagnostic has to say which names are correct.
#[test]
fn importing_a_missing_partial_names_the_ones_that_exist() {
    let world = World::with_shared_template(
        MANIFEST,
        &[(
            "README.md.jinja",
            "{% import 'marcos.jinja' as m %}{{ m.heading(project_name) }}\n",
        )],
        &[(
            "macros.jinja",
            "{% macro heading(text) %}# {{ text }}{% endmacro %}",
        )],
    );

    world
        .init(&[])
        .failure()
        .says("README.md.jinja")
        .says("available partials: macros.jinja");
}

/// A `.jinja` file *inside* the root is an output file, not a partial. Making
/// it importable would give the same file two meanings.
#[test]
fn a_file_inside_the_render_root_is_not_importable() {
    let world = World::with_shared_template(
        MANIFEST,
        &[
            ("header.jinja", "# {{ project_name }}"),
            (
                "README.md.jinja",
                "{% import 'header.jinja' as m %}{{ m }}\n",
            ),
        ],
        &[],
    );

    world.init(&[]).failure().says("defines no partials");
}

/// `{% extends %}`/`{% block %}` share the loader too — it hands MiniJinja
/// whatever bytes a name resolves to, with no distinction between an import,
/// an include and an extends. `{{ super() }}` proves a block can *extend*
/// the parent's content rather than merely replacing it, which is the other
/// half of what makes block inheritance useful.
#[test]
fn a_file_extends_a_shared_base_and_overrides_its_blocks() {
    let world = World::with_shared_template(
        MANIFEST,
        &[(
            "page.html.jinja",
            "{% extends \"base.html.jinja\" %}\n\
             {% block title %}{{ project_name }}{% endblock %}\n\
             {% block content %}{{ super() }} + more{% endblock %}\n",
        )],
        &[(
            "base.html.jinja",
            "{% block title %}Default title{% endblock %}\n\
             {% block content %}Default content{% endblock %}\n",
        )],
    );

    world.init(&[]).success();

    // `title` is overridden outright; `content` extends the base's own
    // content via `super()` rather than replacing it. The overall shape —
    // which block comes first, and the newline between them — comes from the
    // *base*, since a child extending a template contributes only its
    // blocks, nothing else in its own body.
    assert_eq!(
        world.project.read("page.html"),
        "demo\nDefault content + more\n"
    );
}

/// `{% include %}` shares the loader with `{% import %}`.
#[test]
fn include_pulls_in_a_shared_fragment() {
    let world = World::with_shared_template(
        MANIFEST,
        &[("README.md.jinja", "{% include 'header.jinja' %}\nbody\n")],
        &[("header.jinja", "# {{ project_name }}")],
    );

    world.init(&[]).success();

    assert_eq!(world.project.read("README.md"), "# demo\nbody\n");
}

/// The loader reaches the template tree and nothing else. `template.toml` is in
/// that tree but is not a `.jinja` file, so it is not loadable — and neither is
/// anything on the machine running the render.
#[test]
fn a_non_jinja_file_in_the_template_is_not_importable() {
    let world = World::with_shared_template(
        MANIFEST,
        &[("README.md.jinja", "{% include 'template.toml' %}")],
        &[],
    );

    world.init(&[]).failure().says("defines no partials");
}
