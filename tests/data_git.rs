//! Git-hosted data sources, end to end, against real repositories.
//!
//! A `file://` URL is a real clone as far as libgit2 is concerned — the same
//! code path, the same bare temporary clone — so nothing here needs to leave
//! the machine to be honest about what it proves.
//!
//! As with remote data, half of this is about what is *refused*: a clone
//! carries the user's credentials to whatever host a template names, and it is
//! behind the same gate a fetch is.

mod common;

use common::{Repo, file_url, tpl};

const LICENSES_TOML: &str = "ids = [\"MIT\", \"Apache-2.0\"]\ncount = 2\nstrict = true\n";
const LICENSES_JSON: &str = r#"{"ids": ["MIT", "Apache-2.0"], "count": 2, "strict": true}"#;
const LICENSES_YAML: &str = "ids:\n  - MIT\n  - Apache-2.0\ncount: 2\nstrict: true\n";

/// A data repository with the same content in three formats, tagged `v1`.
///
/// A later commit moves the content on the default branch, so a test can tell
/// "read at the tag" from "read at the tip" — which no fixture with a single
/// commit ever could.
fn data_repo(dir: &std::path::Path) -> Repo {
    let data = Repo::init_in(dir, "data");
    data.write("licenses.toml", LICENSES_TOML);
    data.write("licenses.json", LICENSES_JSON);
    data.write("licenses.yaml", LICENSES_YAML);
    data.commit_all("feat: initial");
    data.git(&["tag", "v1"]);

    data.write(
        "licenses.toml",
        "ids = [\"GPL-3.0\"]\ncount = 1\nstrict = true\n",
    );
    data.commit_all("feat: move on");
    data
}

/// A template with one `[data.licenses]` declaration, rendering it into a file.
fn template_with(dir: &std::path::Path, decl: &str, body: &str) -> Repo {
    let template = Repo::init_in(dir, "template");
    template.write(
        "template.toml",
        &format!("name = \"git-data\"\n\n[data.licenses]\n{decl}\n"),
    );
    template.write("template/out.txt.jinja", body);
    template.commit_all("feat: initial");
    template
}

fn project(dir: &std::path::Path) -> Repo {
    let project = Repo::init_in(dir, "project");
    project.write("NOTES.md", "notes\n");
    project.commit_all("chore: initial");
    project
}

/// The body every format test renders: an integer, a boolean and a list, so a
/// parser that stringified everything would fail rather than pass quietly.
const TYPED: &str = "{% if data.licenses.strict %}{{ data.licenses.count + 1 }}:{{ data.licenses.ids | join(',') }}{% endif %}\n";

fn init(project: &Repo, template: &Repo, extra: &[&str]) -> common::Output {
    let source = template.path.to_string_lossy().to_string();
    let mut args = vec!["init", &source, "--defaults"];
    args.extend_from_slice(extra);
    tpl(project, &args)
}

#[test]
fn a_git_data_source_loads_toml_json_and_yaml_through_the_same_parsers() {
    for extension in ["toml", "json", "yaml"] {
        let dir = tempfile::tempdir().unwrap();
        let data = data_repo(dir.path());
        let template = template_with(
            dir.path(),
            &format!(
                "source = \"{}\"\nref = \"v1\"\npath = \"licenses.{extension}\"",
                file_url(&data.path)
            ),
            TYPED,
        );
        let project = project(dir.path());

        init(&project, &template, &["--trust"]).success();

        assert_eq!(
            project.read("out.txt"),
            "3:MIT,Apache-2.0\n",
            "an integer must stay an integer out of a {extension} file in a repository, \
             exactly as it does out of one in the template"
        );
    }
}

#[test]
fn the_url_shorthand_and_the_explicit_keys_load_the_same_data() {
    let dir = tempfile::tempdir().unwrap();
    let data = data_repo(dir.path());
    let template = template_with(
        dir.path(),
        &format!("source = \"{}@v1:licenses.toml\"", file_url(&data.path)),
        TYPED,
    );
    let project = project(dir.path());

    init(&project, &template, &["--trust"]).success();

    assert_eq!(
        project.read("out.txt"),
        "3:MIT,Apache-2.0\n",
        "the shorthand is a spelling, not a second mechanism"
    );
}

#[test]
fn a_git_source_is_read_at_the_declared_ref_and_not_at_the_tip() {
    let dir = tempfile::tempdir().unwrap();
    let data = data_repo(dir.path());
    let template = template_with(
        dir.path(),
        &format!("source = \"{}@v1:licenses.toml\"", file_url(&data.path)),
        "{{ data.licenses.ids | join(',') }}\n",
    );
    let project = project(dir.path());

    init(&project, &template, &["--trust"]).success();

    assert_eq!(
        project.read("out.txt"),
        "MIT,Apache-2.0\n",
        "the tag is the pin; reading the default branch would have given GPL-3.0"
    );
}

#[test]
fn a_git_source_declared_by_sha_resolves_to_that_commit() {
    let dir = tempfile::tempdir().unwrap();
    let data = data_repo(dir.path());
    let sha = data.rev_parse("v1^{commit}");
    let template = template_with(
        dir.path(),
        &format!("source = \"{}@{sha}:licenses.toml\"", file_url(&data.path)),
        "{{ data.licenses.ids | join(',') }}\n",
    );
    let project = project(dir.path());

    init(&project, &template, &["--trust"]).success();

    assert_eq!(project.read("out.txt"), "MIT,Apache-2.0\n");
}

#[test]
fn the_rendered_commit_records_the_resolved_commit_of_a_git_data_source() {
    let dir = tempfile::tempdir().unwrap();
    let data = data_repo(dir.path());
    let sha = data.rev_parse("v1^{commit}");
    let url = file_url(&data.path);
    let template = template_with(
        dir.path(),
        &format!("source = \"{url}@v1:licenses.toml\""),
        "{{ data.licenses.count }}\n",
    );
    let project = project(dir.path());

    init(&project, &template, &["--trust"]).success();

    // The trailer answers "which bytes produced this tree?" from Git alone,
    // which for a moving ref is the only thing that does.
    let message = project.commit_message("refs/tpl/template");
    let expected = format!(
        "Data-Source: licenses = git:{url}@v1:licenses.toml@{}",
        &sha[..7]
    );
    assert!(
        message.contains(&expected),
        "expected `{expected}` in:\n{message}"
    );
}

#[test]
fn two_files_from_one_repository_at_one_ref_clone_it_once() {
    let dir = tempfile::tempdir().unwrap();
    let data = data_repo(dir.path());
    let url = file_url(&data.path);
    let template = Repo::init_in(dir.path(), "template");
    template.write(
        "template.toml",
        &format!(
            "name = \"git-data\"\n\n[data.a]\nsource = \"{url}@v1:licenses.toml\"\n\n\
             [data.b]\nsource = \"{url}@v1:licenses.json\"\n"
        ),
    );
    template.write(
        "template/out.txt.jinja",
        "{{ data.a.count }}{{ data.b.count }}\n",
    );
    template.commit_all("feat: initial");
    let project = project(dir.path());

    init(&project, &template, &["--trust"]).success();

    // Both files load, which is the observable half. The clone cache is keyed
    // by `repo@ref`, so a second clone would be a silent cost rather than a
    // wrong answer — the reason this asserts the result and not a count.
    assert_eq!(project.read("out.txt"), "22\n");
}

#[test]
fn a_git_source_is_refused_when_the_template_is_not_trusted() {
    let dir = tempfile::tempdir().unwrap();
    let data = data_repo(dir.path());
    let template = template_with(
        dir.path(),
        &format!("source = \"{}@v1:licenses.toml\"", file_url(&data.path)),
        "{{ data.licenses.count }}\n",
    );
    let project = project(dir.path());

    // A clone carries the user's credentials to whatever host the template
    // names. Treating it as less than a fetch would make the gate avoidable by
    // choosing a different spelling.
    let output = init(&project, &template, &[]).failure();
    output.says("tpl::data::untrusted");
}

#[test]
fn a_trust_entry_for_the_template_authorises_its_git_data_sources() {
    let dir = tempfile::tempdir().unwrap();
    let data = data_repo(dir.path());
    let template = template_with(
        dir.path(),
        &format!("source = \"{}@v1:licenses.toml\"", file_url(&data.path)),
        "{{ data.licenses.count }}\n",
    );
    let project = project(dir.path());
    project.user_config(&format!(
        "[trust]\ntemplates = [\"{}\"]\n",
        template.path.display().to_string().replace('\\', "/")
    ));

    init(&project, &template, &[]).success();

    assert_eq!(project.read("out.txt"), "2\n");
}

#[test]
fn a_sha256_pin_still_stops_a_git_source_that_changed() {
    let dir = tempfile::tempdir().unwrap();
    let data = data_repo(dir.path());
    let template = template_with(
        dir.path(),
        &format!(
            "source = \"{}@v1:licenses.toml\"\nsha256 = \"{}\"",
            file_url(&data.path),
            "0".repeat(64)
        ),
        "{{ data.licenses.count }}\n",
    );
    let project = project(dir.path());

    let output = init(&project, &template, &["--trust"]).failure();
    output.says("tpl::data::checksum");
}

#[test]
fn a_missing_path_in_the_data_repository_names_the_revision_it_looked_at() {
    let dir = tempfile::tempdir().unwrap();
    let data = data_repo(dir.path());
    let template = template_with(
        dir.path(),
        &format!("source = \"{}@v1:absent.toml\"", file_url(&data.path)),
        "{{ data.licenses.count }}\n",
    );
    let project = project(dir.path());

    let output = init(&project, &template, &["--trust"]).failure();
    // "no such file" without saying *where* it looked sends the author to check
    // a branch they were never reading.
    output.says("tpl::data::load").says("at revision");
}

#[test]
fn an_unresolvable_ref_is_reported_as_a_load_failure_not_a_parse_failure() {
    let dir = tempfile::tempdir().unwrap();
    let data = data_repo(dir.path());
    let template = template_with(
        dir.path(),
        &format!("source = \"{}@v99:licenses.toml\"", file_url(&data.path)),
        "{{ data.licenses.count }}\n",
    );
    let project = project(dir.path());

    let output = init(&project, &template, &["--trust"]).failure();
    output.says("tpl::data::load");
}

#[test]
fn a_git_source_missing_its_path_is_refused_before_anything_is_cloned() {
    let dir = tempfile::tempdir().unwrap();
    let template = template_with(
        dir.path(),
        // A repository that could never be cloned: the point is that we never
        // try, because the declaration is unusable on its own terms.
        "source = \"file:///nowhere/at/all\"\nref = \"v1\"",
        "{{ data.licenses.count }}\n",
    );
    let project = project(dir.path());

    let output = init(&project, &template, &["--trust"]).failure();
    output.says("tpl::data::invalid_git_source").says("`path`");
}

#[test]
fn a_ref_on_a_template_source_is_refused_rather_than_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let template = template_with(
        dir.path(),
        "source = \"data/licenses.toml\"\nkind = \"template\"\nref = \"v1\"\npath = \"x.toml\"",
        "{{ data.licenses.count }}\n",
    );
    let project = project(dir.path());

    let output = init(&project, &template, &["--trust"]).failure();
    output.says("tpl::data::invalid_git_source");
}

#[test]
fn an_scp_style_source_is_not_read_as_a_shorthand() {
    let dir = tempfile::tempdir().unwrap();
    let template = template_with(
        dir.path(),
        // No scheme, so this is not a shorthand and `kind = "git"` has nothing
        // to work with. The error must say to write the keys out, not leave the
        // author guessing why their URL was ignored.
        "source = \"git@example.invalid:acme/data@v1:licenses.toml\"\nkind = \"git\"",
        "{{ data.licenses.count }}\n",
    );
    let project = project(dir.path());

    let output = init(&project, &template, &["--trust"]).failure();
    output
        .says("tpl::data::invalid_git_source")
        .says("`ref` and `path`");
}

#[test]
fn a_ref_that_is_an_expression_loads_after_the_answer_it_depends_on() {
    let dir = tempfile::tempdir().unwrap();
    let data = data_repo(dir.path());
    let template = Repo::init_in(dir.path(), "template");
    template.write(
        "template.toml",
        &format!(
            "name = \"git-data\"\n\n[questions.pin]\ntype = \"string\"\ndefault = \"v1\"\n\n\
             [data.licenses]\nsource = \"{}\"\nref = \"{{{{ pin }}}}\"\npath = \"licenses.toml\"\n",
            file_url(&data.path)
        ),
    );
    template.write(
        "template/out.txt.jinja",
        "{{ data.licenses.ids | join(',') }}\n",
    );
    template.commit_all("feat: initial");
    let project = project(dir.path());

    // The edge from `ref` to `pin` is the whole test: without it the graph
    // could load the source first and resolve a revision named `""`.
    init(&project, &template, &["--trust"]).success();

    assert_eq!(project.read("out.txt"), "MIT,Apache-2.0\n");
}
