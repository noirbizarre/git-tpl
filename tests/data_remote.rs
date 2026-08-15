//! Remote data sources, end to end, against a real HTTP server.
//!
//! Remote data is the one place where a template reaches outside the two
//! repositories involved, so these tests are as much about what is *refused*
//! as about what is fetched.

mod common;

use common::{Repo, TestServer, tpl};

/// A template with one remote data source, rendering it into a file.
fn template_with(dir: &std::path::Path, decl: &str, body: &str) -> Repo {
    let template = Repo::init_in(dir, "template");
    template.write(
        "template.toml",
        &format!("name = \"remote\"\n\n[data.reg]\n{decl}\n"),
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

const REGISTRY: &str = r#"{"count": 2, "items": ["a", "b"], "strict": true}"#;

#[test]
fn a_remote_data_source_is_fetched_and_its_types_are_preserved() {
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();
    let template = template_with(
        dir.path(),
        &format!("source = \"{}\"", server.url("/reg.json")),
        "{% if data.reg.strict %}{{ data.reg.count + 1 }}:{{ data.reg.items | join(',') }}{% endif %}\n",
    );
    let project = project(dir.path());

    tpl(
        &project,
        &[
            "init",
            &template.path.to_string_lossy(),
            "--defaults",
            "--trust",
        ],
    )
    .success();

    assert_eq!(
        project.read("out.txt"),
        "3:a,b\n",
        "an integer must stay an integer across the wire, exactly as for a local file"
    );
}

/// The cache is keyed on the resolved source, so a source used by several
/// questions and several files still costs one request. A template that fetched
/// per use would be slow and, worse, could observe two different responses
/// within one render.
#[test]
fn a_remote_source_is_fetched_once_however_many_times_it_is_used() {
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();

    let template = Repo::init_in(dir.path(), "template");
    template.write(
        "template.toml",
        &format!(
            r#"
name = "remote"

[data.reg]
source = "{}"

[questions.pick]
type = "choice"
prompt = "Pick"
choices_from = "data.reg.items"
default = "a"

[computed]
total = "{{{{ data.reg.count }}}}"
"#,
            server.url("/reg.json")
        ),
    );
    template.write("template/a.txt.jinja", "{{ data.reg.count }}\n");
    template.write("template/b.txt.jinja", "{{ total }}{{ pick }}\n");
    template.commit_all("feat: initial");

    let project = project(dir.path());

    tpl(
        &project,
        &[
            "init",
            &template.path.to_string_lossy(),
            "--defaults",
            "--trust",
        ],
    )
    .success();

    assert_eq!(server.hits(), 1, "one fetch per source per run");
}

/// Non-interactive and untrusted refuses, loudly. A CI runner is the worst
/// possible place to grant a capability by omission, so the render stops and
/// says what would have allowed it.
#[test]
fn a_remote_fetch_is_refused_when_the_template_is_not_trusted() {
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();
    let template = template_with(
        dir.path(),
        &format!("source = \"{}\"", server.url("/reg.json")),
        "{{ data.reg.count }}\n",
    );
    let project = project(dir.path());

    let output = tpl(
        &project,
        &["init", &template.path.to_string_lossy(), "--defaults"],
    )
    .failure();

    output.says("tpl::data::untrusted").says("--trust");
    assert_eq!(
        server.hits(),
        0,
        "refused means not fetched, not fetched-and-discarded"
    );
    assert!(
        !project.exists("out.txt"),
        "nothing is rendered from data that was never loaded"
    );
}

/// `--defaults` means there is nobody to ask, and that has to hold on the
/// `--dry-run` path too. The dry run built its own trust decision inline and
/// left the `--defaults` term out; it survived only because `update` also
/// folds `--defaults` into `tpl.interactive`, so the two spellings happened to
/// agree. Pinned here so that removing either one is a test failure rather
/// than a prompt on a CI runner.
#[test]
fn a_dry_run_under_defaults_refuses_an_untrusted_fetch_like_a_real_run() {
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();
    let template = template_with(
        dir.path(),
        &format!("source = \"{}\"", server.url("/reg.json")),
        "{{ data.reg.count }}\n",
    );
    let project = project(dir.path());

    tpl(
        &project,
        &[
            "init",
            &template.path.to_string_lossy(),
            "--defaults",
            "--trust",
        ],
    )
    .success();

    let before = server.hits();

    tpl(&project, &["update", "--defaults", "--dry-run"])
        .failure()
        .says("tpl::data::untrusted");

    assert_eq!(
        server.hits(),
        before,
        "refused means not fetched, on the dry-run path as much as the real one"
    );
}

#[test]
fn trust_allows_the_fetch_without_a_prompt() {
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();
    let template = template_with(
        dir.path(),
        &format!("source = \"{}\"", server.url("/reg.json")),
        "{{ data.reg.count }}\n",
    );
    let project = project(dir.path());

    tpl(
        &project,
        &[
            "init",
            &template.path.to_string_lossy(),
            "--defaults",
            "--trust",
        ],
    )
    .success();

    assert_eq!(project.read("out.txt"), "2\n");
}

/// A template with no remote sources must never acquire a confirmation it has
/// no use for — which is nearly every template.
#[test]
fn a_template_with_no_remote_sources_is_never_asked_about_trust() {
    let world = common::World::new();

    let output = world.init(&[]).success();

    assert!(
        !output.stderr.contains("remote data source"),
        "no remote sources, no mention of them: {}",
        output.stderr
    );
}

#[test]
fn a_matching_checksum_renders() {
    let digest = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(REGISTRY.as_bytes()));
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();
    let template = template_with(
        dir.path(),
        &format!(
            "source = \"{}\"\nsha256 = \"{digest}\"",
            server.url("/reg.json")
        ),
        "{{ data.reg.count }}\n",
    );
    let project = project(dir.path());

    tpl(
        &project,
        &[
            "init",
            &template.path.to_string_lossy(),
            "--defaults",
            "--trust",
        ],
    )
    .success();

    assert_eq!(project.read("out.txt"), "2\n");
}

/// A pin exists so the render stops when the content changes. Reporting both
/// digests is what lets an author tell "the server changed" from "I pinned the
/// wrong thing".
#[test]
fn a_checksum_mismatch_is_an_error_naming_both_digests() {
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();
    let wrong = "b".repeat(64);
    let template = template_with(
        dir.path(),
        &format!(
            "source = \"{}\"\nsha256 = \"{wrong}\"",
            server.url("/reg.json")
        ),
        "{{ data.reg.count }}\n",
    );
    let project = project(dir.path());

    let output = tpl(
        &project,
        &[
            "init",
            &template.path.to_string_lossy(),
            "--defaults",
            "--trust",
        ],
    )
    .failure();

    output.says("tpl::data::checksum").says(&wrong);
    assert!(!project.exists("out.txt"));
}

/// Nothing pins a remote source except the bytes it returned, so the trailer
/// records the digest whether or not the template asked for one. Without it,
/// `git log` cannot answer "which data produced this tree?".
#[test]
fn the_trailer_records_the_url_and_the_digest() {
    let digest = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(REGISTRY.as_bytes()));
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();
    let url = server.url("/reg.json");
    let template = template_with(
        dir.path(),
        &format!("source = \"{url}\""),
        "{{ data.reg.count }}\n",
    );
    let project = project(dir.path());

    tpl(
        &project,
        &[
            "init",
            &template.path.to_string_lossy(),
            "--defaults",
            "--trust",
        ],
    )
    .success();

    let message = project.commit_message("refs/tpl/template");
    assert!(
        message.contains(&format!("Data-Source: reg = remote:{url}@sha256:{digest}")),
        "expected the URL and the digest in the trailers, got:\n{message}"
    );
}

#[test]
fn an_http_error_status_names_the_status_and_the_source() {
    let server = TestServer::start(vec![("/reg.json", 500, b"boom".to_vec())]);
    let dir = tempfile::tempdir().unwrap();
    let template = template_with(
        dir.path(),
        &format!("source = \"{}\"", server.url("/reg.json")),
        "{{ data.reg.count }}\n",
    );
    let project = project(dir.path());

    let output = tpl(
        &project,
        &[
            "init",
            &template.path.to_string_lossy(),
            "--defaults",
            "--trust",
        ],
    )
    .failure();

    output.says("tpl::data::load").says("reg").says("500");
}

/// The limit is enforced while reading, so a server that lies in
/// `Content-Length` — or sends none at all — is bounded just the same.
#[test]
fn a_response_over_the_size_limit_is_refused() {
    let oversized = vec![b'x'; 6 * 1024 * 1024];
    let server = TestServer::start(vec![("/reg.json", 200, oversized)]);
    let dir = tempfile::tempdir().unwrap();
    let template = template_with(
        dir.path(),
        &format!("source = \"{}\"", server.url("/reg.json")),
        "{{ data.reg.count }}\n",
    );
    let project = project(dir.path());

    let output = tpl(
        &project,
        &[
            "init",
            &template.path.to_string_lossy(),
            "--defaults",
            "--trust",
        ],
    )
    .failure();

    output.says("tpl::data::load").says("limit");
}

/// The confirmation lists every remote source before any is fetched, and it can
/// only do that from the declaration. A source that becomes a URL after an
/// answer is substituted would slip past the list, so it is refused — even with
/// `--trust`, because the point is that the declaration is honest.
#[test]
fn a_source_that_interpolates_to_a_url_is_refused_unless_declared_remote() {
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();

    let template = Repo::init_in(dir.path(), "template");
    template.write(
        "template.toml",
        &format!(
            r#"
name = "sneaky"

[data.reg]
source = "{{{{ base }}}}/reg.json"

[questions.base]
type = "string"
prompt = "Base"
default = "{}"
"#,
            server.base_url()
        ),
    );
    template.write("template/out.txt.jinja", "{{ data.reg.count }}\n");
    template.commit_all("feat: initial");

    let project = project(dir.path());

    let output = tpl(
        &project,
        &[
            "init",
            &template.path.to_string_lossy(),
            "--defaults",
            "--trust",
        ],
    )
    .failure();

    output
        .says("tpl::data::undeclared_remote")
        .says("kind = \"remote\"");
    assert_eq!(server.hits(), 0);
}

/// The same source, declared honestly, does work — otherwise the rule above
/// would be a ban on dynamic URLs rather than a requirement to declare them.
#[test]
fn an_interpolated_url_works_when_it_is_declared_remote() {
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();

    let template = Repo::init_in(dir.path(), "template");
    template.write(
        "template.toml",
        &format!(
            r#"
name = "honest"

[data.reg]
source = "{{{{ base }}}}/reg.json"
kind = "remote"
format = "json"

[questions.base]
type = "string"
prompt = "Base"
default = "{}"
"#,
            server.base_url()
        ),
    );
    template.write("template/out.txt.jinja", "{{ data.reg.count }}\n");
    template.commit_all("feat: initial");

    let project = project(dir.path());

    tpl(
        &project,
        &[
            "init",
            &template.path.to_string_lossy(),
            "--defaults",
            "--trust",
        ],
    )
    .success();

    assert_eq!(project.read("out.txt"), "2\n");
}

/// `kind = "remote"` can be declared on any string, so the scheme is checked
/// rather than assumed from the inference that an explicit kind bypasses.
#[test]
fn only_http_and_https_are_fetched() {
    let dir = tempfile::tempdir().unwrap();
    let template = template_with(
        dir.path(),
        "source = \"file:///etc/passwd\"\nkind = \"remote\"",
        "{{ data.reg.count }}\n",
    );
    let project = project(dir.path());

    let output = tpl(
        &project,
        &[
            "init",
            &template.path.to_string_lossy(),
            "--defaults",
            "--trust",
        ],
    )
    .failure();

    output.says("tpl::data::load").says("scheme");
}
