//! `[extends]` — template inheritance, end to end.
//!
//! See `docs/adr/034-template-inheritance.md`.

mod common;

use common::{Repo, Template, tpl, tpl_outside};

/// A scratch directory that is deliberately not a repository, for the
/// project-free `render`/`questions`/`context` commands.
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

    fn run(&self, args: &[&str]) -> common::Output {
        tpl_outside(self.dir.path(), self.config.path(), args)
    }

    fn json(&self, args: &[&str]) -> serde_json::Value {
        let mut full = vec!["--json"];
        full.extend_from_slice(args);
        self.run(&full).success().json()
    }

    fn out(&self) -> std::path::PathBuf {
        self.dir.path().join("out")
    }

    fn render(&self, source: &str) -> common::Output {
        self.run(&[
            "render",
            source,
            "--output",
            self.out().to_str().unwrap(),
            "--defaults",
        ])
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.out().join(path))
            .unwrap_or_else(|e| panic!("read {path}: {e}"))
    }
}

/// A base template with two questions, one file, and a computed value.
const BASE_MANIFEST: &str = r#"
name = "base"
description = "The shared base"

[questions.project_name]
type = "string"
prompt = "Project name"
default = "demo"

[questions.license]
type = "choice"
prompt = "License"
choices = ["MIT", "Apache-2.0"]
default = "MIT"

[computed]
package_name = "{{ project_name | lower }}"
"#;

const BASE_FILES: &[(&str, &str)] = &[
    (
        "README.md.jinja",
        "# {{ project_name }}\n\nLicense: {{ license }}\n",
    ),
    (".github/workflows/ci.yml.jinja", "name: CI\n# from base\n"),
];

fn base(dir: &std::path::Path) -> Template {
    Template::with_shared(dir, BASE_MANIFEST, BASE_FILES, &[])
}

/// The acceptance criterion in issue #28, almost verbatim: a child overriding
/// one question and one file renders a tree identical to the parent's except
/// in those two places, deterministically.
#[test]
fn a_child_overriding_one_question_and_one_file_matches_the_parent_elsewhere() {
    let dir = tempfile::tempdir().unwrap();
    let parent = base(dir.path());
    let child = Template::extending(
        "child",
        &parent,
        "v1.0.0",
        r#"
        [questions.license]
        type = "choice"
        prompt = "License"
        choices = ["MIT", "Apache-2.0"]
        default = "Apache-2.0"
        "#,
        &[(
            "README.md.jinja",
            "# {{ project_name }}\n\nOverridden license: {{ license }}\n",
        )],
    );

    let scratch = Scratch::new();
    scratch.render(&child.source()).success();

    // Overridden in the child.
    assert_eq!(
        scratch.read("README.md"),
        "# demo\n\nOverridden license: Apache-2.0\n"
    );
    // Untouched: inherited from the parent, unchanged.
    assert_eq!(
        scratch.read(".github/workflows/ci.yml"),
        "name: CI\n# from base\n"
    );

    // Deterministic: rendering twice produces the same bytes.
    let out2 = dir.path().join("out2");
    scratch
        .run(&[
            "render",
            &child.source(),
            "--output",
            out2.to_str().unwrap(),
            "--defaults",
        ])
        .success();
    assert_eq!(
        std::fs::read(scratch.out().join(".github/workflows/ci.yml")).unwrap(),
        std::fs::read(out2.join(".github/workflows/ci.yml")).unwrap()
    );
}

/// A template with no `[extends]` is unaffected -- the chain-of-one identity.
#[test]
fn a_template_with_no_extends_renders_exactly_as_before() {
    let dir = tempfile::tempdir().unwrap();
    let parent = base(dir.path());
    let scratch = Scratch::new();

    scratch.render(&parent.source()).success();

    assert_eq!(scratch.read("README.md"), "# demo\n\nLicense: MIT\n");
}

/// Ancestor questions come first, in ancestor order, then the child's own new
/// ones.
#[test]
fn ancestor_questions_come_before_the_new_ones_the_child_adds() {
    let dir = tempfile::tempdir().unwrap();
    let parent = base(dir.path());
    let child = Template::extending(
        "child",
        &parent,
        "v1.0.0",
        r#"
        [questions.extra]
        type = "string"
        default = "x"
        "#,
        &[],
    );

    let scratch = Scratch::new();
    let json = scratch.json(&["questions", &child.source()]);
    let names: Vec<String> = json["questions"]
        .as_array()
        .expect("questions")
        .iter()
        .map(|q| q["name"].as_str().unwrap().to_string())
        .collect();

    assert_eq!(names, ["project_name", "license", "extra"]);
}

/// An overridden question keeps the parent's position -- inserting an
/// override must not reshuffle the prompt sequence.
#[test]
fn an_overridden_question_keeps_the_parents_position() {
    let dir = tempfile::tempdir().unwrap();
    let parent = base(dir.path());
    let child = Template::extending(
        "child",
        &parent,
        "v1.0.0",
        r#"
        [questions.project_name]
        type = "string"
        prompt = "Project name (overridden)"
        default = "overridden"
        "#,
        &[],
    );

    let scratch = Scratch::new();
    let json = scratch.json(&["questions", &child.source()]);
    let names: Vec<String> = json["questions"]
        .as_array()
        .expect("questions")
        .iter()
        .map(|q| q["name"].as_str().unwrap().to_string())
        .collect();

    // Still first, not moved to the end for having been redeclared.
    assert_eq!(names, ["project_name", "license"]);
}

/// `[data]` and `[computed]` merge by name too, the same rule as `[questions]`.
#[test]
fn data_and_computed_merge_by_name_across_the_chain() {
    let dir = tempfile::tempdir().unwrap();
    let parent = Template::with_shared(
        dir.path(),
        r#"
        name = "base"

        [data.licenses]
        source = "data/licenses.toml"

        [questions.project_name]
        type = "string"
        default = "demo"

        [computed]
        package_name = "{{ project_name | lower }}"
        "#,
        &[(
            "out.txt.jinja",
            "{{ package_name }} / {{ data.licenses.ids | join(',') }}\n",
        )],
        &[("data/licenses.toml", "ids = [\"MIT\"]\n")],
    );
    let child = Template::extending("child", &parent, "v1.0.0", "", &[]);

    let scratch = Scratch::new();
    scratch.render(&child.source()).success();
    assert_eq!(scratch.read("out.txt"), "demo / MIT\n");
}

/// A leaf overriding a data source by name reads its own file, not the
/// parent's.
#[test]
fn a_leaf_overriding_a_data_source_reads_its_own_file() {
    let dir = tempfile::tempdir().unwrap();
    let parent = Template::with_shared(
        dir.path(),
        r#"
        name = "base"
        [data.licenses]
        source = "data/licenses.toml"
        "#,
        &[("out.txt.jinja", "{{ data.licenses.ids | join(',') }}\n")],
        &[("data/licenses.toml", "ids = [\"MIT\"]\n")],
    );
    let child = Template::extending(
        "child",
        &parent,
        "v1.0.0",
        r#"
        [data.licenses]
        source = "data/licenses.toml"
        "#,
        &[],
    );
    child
        .repo
        .write("data/licenses.toml", "ids = [\"Apache-2.0\"]\n");
    child.repo.commit_all("feat: override licenses data");

    let scratch = Scratch::new();
    scratch.render(&child.source()).success();
    assert_eq!(scratch.read("out.txt"), "Apache-2.0\n");
}

/// `remove` drops an inherited file from the merge.
#[test]
fn remove_drops_an_inherited_file() {
    let dir = tempfile::tempdir().unwrap();
    let parent = base(dir.path());
    let child = Template::extending(
        "child",
        &parent,
        "v1.0.0",
        r#"remove = ["template/.github/workflows/ci.yml.jinja"]"#,
        &[],
    );

    let scratch = Scratch::new();
    scratch.render(&child.source()).success();

    assert!(scratch.read("README.md").starts_with("# demo"));
    assert!(!scratch.out().join(".github/workflows/ci.yml").exists());
}

/// `remove` naming a path the parent does not have is an error.
#[test]
fn remove_of_a_path_the_parent_does_not_have_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let parent = base(dir.path());
    let child = Template::extending(
        "child",
        &parent,
        "v1.0.0",
        r#"remove = ["template/does-not-exist.txt"]"#,
        &[],
    );

    let scratch = Scratch::new();
    let output = scratch.run(&[
        "--json",
        "render",
        &child.source(),
        "--output",
        scratch.out().to_str().unwrap(),
        "--defaults",
    ]);
    assert_eq!(output.error_code(), "tpl::extends::remove_missing");
}

/// A parent pinned to a branch, rather than a tag or a commit, is rejected.
#[test]
fn an_unpinned_parent_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let parent = base(dir.path());
    let child = Repo::init_in(dir.path(), "child");
    child.write("template/dummy.txt", "x");
    child.write(
        "template.toml",
        &format!(
            "name = \"child\"\n\n[extends]\nsource = \"{}\"\nrev = \"main\"\n",
            parent.source()
        ),
    );
    child.commit_all("feat: initial child template");

    let scratch = Scratch::new();
    let source = child.path.to_string_lossy().into_owned();
    let output = scratch.run(&[
        "--json",
        "render",
        &source,
        "--output",
        scratch.out().to_str().unwrap(),
        "--defaults",
    ]);
    assert_eq!(output.error_code(), "tpl::extends::unpinned");
}

/// A parent pinned to a raw commit SHA -- not a tag -- is accepted.
#[test]
fn a_parent_pinned_to_a_commit_sha_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let parent = base(dir.path());
    let sha = parent.repo.rev_parse("HEAD");
    let child = Repo::init_in(dir.path(), "child");
    child.write("template/dummy.txt", "x");
    child.write(
        "template.toml",
        &format!(
            "name = \"child\"\n\n[extends]\nsource = \"{}\"\nrev = \"{sha}\"\n",
            parent.source()
        ),
    );
    child.commit_all("feat: initial child template");

    let scratch = Scratch::new();
    scratch.render(&child.path.to_string_lossy()).success();
    assert!(scratch.read("README.md").starts_with("# demo"));
}

/// A chain that revisits a template it has already resolved is rejected up
/// front, by name.
#[test]
fn a_cyclic_chain_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repo::init_in(dir.path(), "template");
    repo.write("template.toml", "name = \"a\"\n");
    repo.write("template/dummy.txt", "x");
    repo.commit_all("feat: v1, no extends");
    repo.git(&["tag", "v1"]);

    let source = repo.path.to_string_lossy().into_owned();
    repo.write(
        "template.toml",
        &format!("name = \"a\"\n\n[extends]\nsource = \"{source}\"\nrev = \"v1\"\n"),
    );
    repo.commit_all("feat: v2, extends v1");
    repo.git(&["tag", "v2"]);

    // Move `v1` to point at `v2`'s own commit, so resolving `v2`'s
    // `[extends]` (`rev = "v1"`) now resolves back to `v2` itself -- the
    // simplest genuine cycle, without a second repository.
    repo.git(&["tag", "-f", "v1", "v2"]);

    let scratch = Scratch::new();
    let output = scratch.run(&[
        "--json",
        "render",
        &source,
        "--ref",
        "v2",
        "--output",
        scratch.out().to_str().unwrap(),
        "--defaults",
    ]);
    assert_eq!(output.error_code(), "tpl::extends::cycle");
}

/// A chain deeper than the limit is rejected.
#[test]
fn a_chain_deeper_than_the_limit_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut current = Template::with_shared(
        dir.path(),
        "name = \"layer0\"\n",
        &[("dummy.txt", "x")],
        &[],
    );

    // 9 ancestors above the leaf is one past the limit of 8.
    for i in 1..=9 {
        let name = format!("layer{i}");
        current = Template::extending(&name, &current, "v1", "", &[("dummy.txt", "x")]);
    }

    let scratch = Scratch::new();
    let output = scratch.run(&[
        "--json",
        "render",
        &current.source(),
        "--output",
        scratch.out().to_str().unwrap(),
        "--defaults",
    ]);
    assert_eq!(output.error_code(), "tpl::extends::depth");
}

/// A name declared as a question by one layer and a computed value by another
/// is rejected, once the chain is merged.
#[test]
fn a_cross_layer_kind_collision_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let parent = Template::with_shared(
        dir.path(),
        r#"
        name = "base"
        [questions.shared]
        type = "string"
        default = "x"
        "#,
        &[("dummy.txt", "x")],
        &[],
    );
    let child = Template::extending(
        "child",
        &parent,
        "v1.0.0",
        r#"
        [computed]
        shared = "{{ 1 }}"
        "#,
        &[("dummy.txt", "x")],
    );

    let scratch = Scratch::new();
    let output = scratch.run(&[
        "--json",
        "render",
        &child.source(),
        "--output",
        scratch.out().to_str().unwrap(),
        "--defaults",
    ]);
    assert_eq!(output.error_code(), "tpl::manifest::extends_kind_collision");
}

/// A bare partial reference resolves to the nearest layer's own declaration,
/// and `parent:` reaches the one it shadowed.
#[test]
fn parent_prefix_reaches_the_shadowed_partial() {
    let dir = tempfile::tempdir().unwrap();
    let parent = Template::with_shared(
        dir.path(),
        "name = \"base\"\n",
        &[("uses.txt.jinja", "{% include \"macros.jinja\" %}\n")],
        &[("macros.jinja", "from the base")],
    );
    let child = Template::extending(
        "child",
        &parent,
        "v1.0.0",
        "",
        &[(
            "both.txt.jinja",
            "bare: {% include \"macros.jinja\" %}\nparent: {% include \"parent:macros.jinja\" %}\n",
        )],
    );
    child.repo.write("macros.jinja", "from the child");
    child.repo.commit_all("feat: add child macro");

    let scratch = Scratch::new();
    scratch.render(&child.source()).success();

    // Bare `uses.txt`, inherited unmodified from the parent, still resolves
    // within the merged namespace -- the nearest (child's own) declaration.
    assert_eq!(scratch.read("uses.txt"), "from the child\n");
    assert_eq!(
        scratch.read("both.txt"),
        "bare: from the child\nparent: from the base\n"
    );
}

/// The whole ancestor chain is recorded in the rendered commit, nearest
/// parent first, and `git tpl status` can read it back — both directly, via
/// its own `renderedExtends` field, and via the raw trailers underneath it.
#[test]
fn the_rendered_commit_records_the_whole_ancestor_chain() {
    let dir = tempfile::tempdir().unwrap();
    let grandparent = Template::with_shared(
        dir.path(),
        "name = \"grandparent\"\n",
        &[("dummy.txt", "x")],
        &[],
    );
    let parent = Template::extending("parent", &grandparent, "v1.0.0", "", &[("dummy.txt", "x")]);
    let child = Template::extending("child", &parent, "v1.0.0", "", &[("dummy.txt", "x")]);

    let project = Repo::init_in(dir.path(), "project");
    project.write("NOTES.md", "notes\n");
    project.commit_all("chore: initial commit");

    tpl(&project, &["init", &child.source(), "--defaults"]).success();

    let status = tpl(&project, &["--json", "status"]).success().json();
    let ref_name = status["ref"].as_str().expect("ref name");
    let message = project.commit_message(ref_name);

    assert!(message.contains("Template-Source:"), "{message}");
    let parent_line = message
        .lines()
        .find(|l| l.starts_with("Template-Extends:") && l.contains(&parent.source()));
    let grandparent_line = message
        .lines()
        .find(|l| l.starts_with("Template-Extends:") && l.contains(&grandparent.source()));
    assert!(parent_line.is_some(), "{message}");
    assert!(grandparent_line.is_some(), "{message}");

    // Nearest parent first, root ancestor last.
    let lines: Vec<&str> = message
        .lines()
        .filter(|l| l.starts_with("Template-Extends:"))
        .collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains(&parent.source()), "{lines:?}");
    assert!(lines[1].contains(&grandparent.source()), "{lines:?}");

    // The same chain, structured, without a caller having to parse trailers
    // at all.
    let rendered_extends = status["renderedExtends"]
        .as_array()
        .expect("renderedExtends");
    assert_eq!(rendered_extends.len(), 2);
    assert_eq!(rendered_extends[0]["source"], parent.source());
    assert_eq!(rendered_extends[1]["source"], grandparent.source());
}

/// The text report names the chain too, only when there is one.
#[test]
fn status_text_names_the_extends_chain_only_when_there_is_one() {
    let dir = tempfile::tempdir().unwrap();
    let parent = base(dir.path());
    let child = Template::extending("child", &parent, "v1.0.0", "", &[("dummy.txt", "x")]);

    let project = Repo::init_in(dir.path(), "project");
    project.write("NOTES.md", "notes\n");
    project.commit_all("chore: initial commit");

    tpl(&project, &["init", &parent.source(), "--defaults"]).success();
    let plain = tpl(&project, &["status"]).all();
    assert!(!plain.contains("Extends:"), "{plain}");

    // A second project, this time attached to the extending template.
    let project2 = Repo::init_in(dir.path(), "project2");
    project2.write("NOTES.md", "notes\n");
    project2.commit_all("chore: initial commit");
    tpl(&project2, &["init", &child.source(), "--defaults"]).success();
    let plain = tpl(&project2, &["status"]).all();
    assert!(plain.contains("Extends:"), "{plain}");
    assert!(plain.contains(&parent.source()), "{plain}");
}

/// A template with no `[extends]` reports an empty chain, not a missing
/// field -- a script must be able to index `renderedExtends` unconditionally.
#[test]
fn status_reports_an_empty_chain_for_a_template_with_no_parent() {
    let dir = tempfile::tempdir().unwrap();
    let template = base(dir.path());

    let project = Repo::init_in(dir.path(), "project");
    project.write("NOTES.md", "notes\n");
    project.commit_all("chore: initial commit");

    tpl(&project, &["init", &template.source(), "--defaults"]).success();

    let status = tpl(&project, &["--json", "status"]).success().json();
    assert_eq!(status["renderedExtends"], serde_json::json!([]));
}

/// `git tpl context --json` reports the ancestor chain, and which layer
/// declared each inherited question and data source -- so a chain several
/// layers deep can be debugged without cloning every ancestor by hand.
#[test]
fn context_reports_the_chain_and_which_layer_declared_what() {
    let dir = tempfile::tempdir().unwrap();
    let parent = Template::with_shared(
        dir.path(),
        r#"
        name = "base"
        [data.licenses]
        source = "data/licenses.toml"
        [questions.inherited]
        type = "string"
        default = "from the base"
        "#,
        &[("dummy.txt", "x")],
        &[("data/licenses.toml", "ids = [\"MIT\"]\n")],
    );
    let child = Template::extending(
        "child",
        &parent,
        "v1.0.0",
        r#"
        [questions.own]
        type = "string"
        default = "child's own"
        "#,
        &[("dummy.txt", "x")],
    );

    let scratch = Scratch::new();
    let json = scratch.json(&["context", &child.source(), "--defaults"]);

    let chain = json["extends"]["chain"].as_array().expect("chain");
    assert_eq!(chain.len(), 1);
    assert_eq!(chain[0]["source"], parent.source());

    // Declared by the parent, at index 0 of the chain above.
    assert_eq!(json["extends"]["questions"]["inherited"], 0);
    assert_eq!(json["extends"]["data"]["licenses"], 0);
    // The child's own question is absent -- it needs no origin, it is the
    // template actually resolved.
    assert!(json["extends"]["questions"].get("own").is_none());
}

/// A template with no `[extends]` reports an empty chain and no origins --
/// the common case, and it must cost nothing to check.
#[test]
fn context_reports_an_empty_chain_for_a_template_with_no_parent() {
    let dir = tempfile::tempdir().unwrap();
    let template = base(dir.path());

    let scratch = Scratch::new();
    let json = scratch.json(&["context", &template.source(), "--defaults"]);

    assert_eq!(json["extends"]["chain"], serde_json::json!([]));
    assert_eq!(json["extends"]["questions"], serde_json::json!({}));
    assert_eq!(json["extends"]["data"], serde_json::json!({}));
}

/// `git tpl lint` still succeeds on an extending template -- it checks the
/// leaf's own files against the merged manifest.
#[test]
fn lint_succeeds_on_an_extending_template() {
    let dir = tempfile::tempdir().unwrap();
    let parent = base(dir.path());
    let child = Template::extending("child", &parent, "v1.0.0", "", &[("dummy.txt", "x")]);

    let scratch = Scratch::new();
    scratch.run(&["lint", &child.source()]).success();
}
