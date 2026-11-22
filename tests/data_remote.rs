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

fn registry_digest() -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(REGISTRY.as_bytes()))
}

fn render_cached(template: &Repo, cache: &std::path::Path, extra: &[&str]) -> common::Output {
    let output = template.path.join("out");
    let mut args = vec![
        "--json",
        "--cache-dir",
        cache.to_str().unwrap(),
        "render",
        template.path.to_str().unwrap(),
        "--defaults",
        "--force",
        "-o",
        output.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    tpl(template, &args)
}

#[test]
fn pinned_http_is_reused_across_invocations_and_urls_even_offline() {
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let digest = registry_digest();
    let template = template_with(
        dir.path(),
        &format!(
            "source = '{}'\nsha256 = ' {} '",
            server.url("/reg.json"),
            digest.to_uppercase()
        ),
        "{{ data.reg.count }}",
    );
    render_cached(&template, cache.path(), &["--trust"]).success();
    render_cached(&template, cache.path(), &["--trust"]).success();
    assert_eq!(server.hits(), 1);
    assert_eq!(template.read("out/out.txt"), "2");

    // A different URL cannot affect immutable content identity or require its old server.
    template.write("template.toml", &format!("name = 'remote'\n[data.reg]\nsource = 'http://127.0.0.1:1/other.json'\nsha256 = '{digest}'\n"));
    template.commit_all("change content location");
    drop(server);
    for extra in [vec!["--trust"], vec!["--trust", "--offline"]] {
        render_cached(&template, cache.path(), &extra).success();
        assert_eq!(template.read("out/out.txt"), "2");
    }
}

#[test]
fn pinned_http_offline_misses_explain_how_to_populate_the_cache() {
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let template = template_with(
        dir.path(),
        &format!(
            "source = '{}'\nsha256 = '{}'",
            server.url("/reg.json"),
            registry_digest()
        ),
        "{{ data.reg.count }}",
    );
    let result = render_cached(&template, cache.path(), &["--trust", "--offline"]).failure();
    assert_eq!(result.error_code(), "tpl::data::offline_miss");
    assert!(result.json().to_string().contains("populate the cache"));
    assert_eq!(server.hits(), 0);
    assert!(!template.path.join("out").exists());
}

#[test]
fn corrupt_http_objects_fail_without_network_fallback_and_can_be_cleared() {
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let digest = registry_digest();
    let template = template_with(
        dir.path(),
        &format!(
            "source = '{}'\nsha256 = '{digest}'",
            server.url("/reg.json")
        ),
        "{{ data.reg.count }}",
    );
    let entry = cache.path().join("v1/http").join(&digest);
    for corruption in ["checksum", "oversized", "missing body", "metadata"] {
        render_cached(&template, cache.path(), &["--trust"]).success();
        match corruption {
            "checksum" => std::fs::write(entry.join("body"), b"{}").unwrap(),
            "oversized" => {
                std::fs::write(entry.join("body"), vec![b' '; 5 * 1024 * 1024 + 1]).unwrap()
            }
            "missing body" => std::fs::remove_file(entry.join("body")).unwrap(),
            _ => std::fs::write(entry.join("git-tpl-cache.json"), b"invalid").unwrap(),
        }
        let hits = server.hits();
        for flags in [vec!["--trust"], vec!["--trust", "--offline"]] {
            let result = render_cached(&template, cache.path(), &flags).failure();
            assert_eq!(result.error_code(), "tpl::cache::http_corrupt");
            assert!(
                result
                    .json()
                    .to_string()
                    .contains(&format!("cache clear sha256:{digest}"))
            );
        }
        assert_eq!(server.hits(), hits);
        tpl(
            &template,
            &[
                "--cache-dir",
                cache.path().to_str().unwrap(),
                "cache",
                "clear",
                &format!("sha256:{digest}"),
            ],
        )
        .success();
        assert!(!entry.exists());
    }
    render_cached(&template, cache.path(), &["--trust"]).success();
}

#[test]
fn cached_http_content_does_not_bypass_trust_scheme_or_parser_checks() {
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let digest = registry_digest();
    let template = template_with(
        dir.path(),
        &format!(
            "source = '{}'\nsha256 = '{digest}'",
            server.url("/reg.json")
        ),
        "{{ data.reg.count }}",
    );
    render_cached(&template, cache.path(), &["--trust"]).success();
    assert_eq!(
        render_cached(&template, cache.path(), &["--offline"])
            .failure()
            .error_code(),
        "tpl::data::untrusted"
    );
    for (source, format, code) in [
        ("file:///reg.json".to_owned(), "json", "tpl::data::load"),
        (server.url("/reg.json"), "toml", "tpl::data::parse"),
    ] {
        template.write("template.toml", &format!("name = 'remote'\n[data.reg]\nkind = 'remote'\nsource = '{source}'\nformat = '{format}'\nsha256 = '{digest}'\n"));
        template.commit_all("change declaration validation");
        assert_eq!(
            render_cached(&template, cache.path(), &["--trust", "--offline"])
                .failure()
                .error_code(),
            code
        );
    }
    assert_eq!(server.hits(), 1);
}

#[test]
fn an_unpinned_alias_can_populate_a_pin_but_cannot_gain_persistent_offline_semantics() {
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let digest = registry_digest();
    let url = server.url("/reg.json");
    let template = template_with(
        dir.path(),
        &format!("source = '{url}'\n[data.pin]\nsource = '{url}'\nsha256 = '{digest}'"),
        "{{ data.reg.count }}{{ data.pin.count }}",
    );
    for hits in [1, 2] {
        render_cached(&template, cache.path(), &["--trust"]).success();
        assert_eq!(server.hits(), hits);
    }
    assert_eq!(
        std::fs::read(cache.path().join("v1/http").join(&digest).join("body")).unwrap(),
        REGISTRY.as_bytes()
    );
    // Force the pinned declaration first: its memory bytes must not authorize an unpinned offline read.
    template.write(
        "template/out.txt.jinja",
        "{{ data.pin.count }}{{ data.reg.count }}",
    );
    template.commit_all("read pinned alias first");
    assert_eq!(
        render_cached(&template, cache.path(), &["--trust", "--offline"])
            .failure()
            .error_code(),
        "tpl::data::offline"
    );
    assert_eq!(server.hits(), 2);
}

#[test]
fn a_downloaded_checksum_mismatch_never_publishes_an_http_object() {
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let digest = "0".repeat(64);
    let template = template_with(
        dir.path(),
        &format!(
            "source = '{}'\nsha256 = '{digest}'",
            server.url("/reg.json")
        ),
        "{{ data.reg.count }}",
    );
    assert_eq!(
        render_cached(&template, cache.path(), &["--trust"])
            .failure()
            .error_code(),
        "tpl::data::checksum"
    );
    assert!(!cache.path().join("v1/http").join(digest).exists());
}

#[test]
fn independent_render_processes_publish_one_complete_http_object() {
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let digest = registry_digest();
    let template = template_with(
        dir.path(),
        &format!(
            "source = '{}'\nsha256 = '{digest}'",
            server.url("/reg.json")
        ),
        "{{ data.reg.count }}",
    );
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(6));
    let handles: Vec<_> = (0..6)
        .map(|index| {
            let path = template.path.clone();
            let config = template.config_home().to_owned();
            let root = cache.path().to_owned();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let output = root.join(format!("out-{index}"));
                barrier.wait();
                common::tpl_outside(
                    &path,
                    &config,
                    &[
                        "--cache-dir",
                        root.to_str().unwrap(),
                        "render",
                        path.to_str().unwrap(),
                        "--trust",
                        "--defaults",
                        "-o",
                        output.to_str().unwrap(),
                    ],
                )
                .success();
                assert_eq!(
                    std::fs::read_to_string(output.join("out.txt")).unwrap(),
                    "2"
                );
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(server.hits(), 1);
    assert_eq!(
        std::fs::read(cache.path().join("v1/http").join(digest).join("body")).unwrap(),
        REGISTRY.as_bytes()
    );
}

#[test]
fn offline_http_reuse_preserves_provenance_and_update_working_state() {
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let digest = registry_digest();
    let url = server.url("/reg.json");
    let template = template_with(
        dir.path(),
        &format!("source = '{url}'\nsha256 = '{digest}'"),
        "{{ data.reg.count }}",
    );
    let project = project(dir.path());
    tpl(
        &project,
        &[
            "--cache-dir",
            cache.path().to_str().unwrap(),
            "init",
            template.path.to_str().unwrap(),
            "--trust",
            "--defaults",
        ],
    )
    .success();
    let reference = project.git(&["for-each-ref", "--format=%(refname)", "refs/tpl/"]);
    let reference = reference.trim();
    let tip = project.rev_parse(reference);
    let message = project.git(&["log", "-1", "--format=%B", reference]);
    assert!(message.contains(&format!("Data-Source: reg = remote:{url}@sha256:{digest}")));
    let state = project.working_state();
    drop(server);
    tpl(
        &project,
        &[
            "--offline",
            "--cache-dir",
            cache.path().to_str().unwrap(),
            "update",
            "--trust",
            "--defaults",
        ],
    )
    .success();
    assert_eq!(
        project.rev_parse(reference),
        tip,
        "cache state cannot create a render commit"
    );
    assert_eq!(project.working_state(), state);
}

#[test]
fn unused_pinned_http_data_does_not_require_usable_cache_storage() {
    let dir = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let blocked = cache.path().join("not-a-directory");
    std::fs::write(&blocked, b"no storage").unwrap();
    let template = template_with(
        dir.path(),
        &format!(
            "source = 'http://127.0.0.1:1/never.json'\nsha256 = '{}'",
            registry_digest()
        ),
        "{% if false %}{{ data.reg.count }}{% endif %}unused",
    );
    render_cached(&template, &blocked, &["--offline"]).success();
    assert_eq!(template.read("out/out.txt"), "unused");
}

#[test]
fn aliases_share_bytes_but_validate_and_record_each_declaration() {
    for (policy, expected) in [
        ("", None),
        (
            "sha256 = '0000000000000000000000000000000000000000000000000000000000000000'",
            Some("tpl::data::checksum"),
        ),
        ("format = 'toml'", Some("tpl::data::parse")),
    ] {
        let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
        let dir = tempfile::tempdir().unwrap();
        let template = template_with(
            dir.path(),
            &format!(
                "source = '{}'\n[data.second]\nsource = '{}'\n{policy}",
                server.url("/reg.json"),
                server.url("/reg.json")
            ),
            "{{ data.reg.count }}{{ data.second.count }}",
        );
        let project = project(dir.path());
        let result = tpl(
            &project,
            &[
                "--json",
                "init",
                template.path.to_str().unwrap(),
                "--defaults",
                "--trust",
            ],
        );
        if let Some(code) = expected {
            assert_eq!(result.failure().error_code(), code);
            assert!(!project.path.join(".config/git.tpl.toml").exists());
        } else {
            result.success();
            let reference = project.git(&["for-each-ref", "--format=%(refname)", "refs/tpl/"]);
            let message = project.git(&["log", "-1", "--format=%B", reference.trim()]);
            assert!(message.contains("Data-Source: reg = remote:"));
            assert!(message.contains("Data-Source: second = remote:"));
        }
        assert_eq!(server.hits(), 1);
    }
}

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

// --- the persistent trust list ----------------------------------------------

/// A `[trust]` entry is prior consent, deliberately written, and no weaker than
/// `--trust` — so it grants where `--trust` would, including where there is
/// nobody to ask.
///
/// The alternative is `--trust` on every CI invocation as the only way to use a
/// template you have already agreed to, which teaches people to pass `--trust`
/// unconditionally.
#[test]
fn a_trusted_pattern_grants_without_a_prompt_and_without_a_flag() {
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();
    let template = template_with(
        dir.path(),
        &format!("source = \"{}\"", server.url("/reg.json")),
        "{{ data.reg.count }}\n",
    );
    let project = project(dir.path());
    // Matched against the *template's* source, not the data URL: trust is a
    // statement about whose template you are willing to run.
    //
    // Forward slashes: a Windows path written into TOML raw is a string full of
    // invalid escape sequences. `normalise` treats both separators alike, so
    // the pattern still matches the backslashed source the command is given.
    let source = template.path.to_string_lossy().into_owned();
    project.user_config(&format!(
        "[trust]\ntemplates = [\"{}/**\"]\n",
        dir.path().to_string_lossy().replace('\\', "/")
    ));

    // No `--trust`, and `--defaults` means there is nobody to ask.
    tpl(&project, &["init", &source, "--defaults"]).success();

    assert_eq!(project.read("out.txt"), "2\n");
    assert_eq!(server.hits(), 1);
}

/// The other half of the same claim: nothing is granted by omission. A template
/// the list does not name is still refused, loudly, and still says what would
/// have allowed it.
#[test]
fn a_template_outside_the_trust_list_is_still_refused() {
    let server = TestServer::start(vec![("/reg.json", 200, REGISTRY.into())]);
    let dir = tempfile::tempdir().unwrap();
    let template = template_with(
        dir.path(),
        &format!("source = \"{}\"", server.url("/reg.json")),
        "{{ data.reg.count }}\n",
    );
    let project = project(dir.path());
    project.user_config("[trust]\ntemplates = [\"github.com/someone-else/*\"]\n");

    tpl(
        &project,
        &["init", &template.path.to_string_lossy(), "--defaults"],
    )
    .failure()
    .says("tpl::data::untrusted")
    .says("--trust");

    assert_eq!(server.hits(), 0);
}

/// Trust gates the network and nothing else. A template with no remote data is
/// unaffected by the list, in either direction.
#[test]
fn an_empty_trust_list_changes_nothing_for_a_template_with_no_remote_data() {
    let dir = tempfile::tempdir().unwrap();
    let template = Repo::init_in(dir.path(), "template");
    template.write("template.toml", "name = \"plain\"\n");
    template.write("template/out.txt.jinja", "plain\n");
    template.commit_all("feat: initial");
    let project = project(dir.path());
    project.user_config("[trust]\ntemplates = []\n");

    tpl(
        &project,
        &["init", &template.path.to_string_lossy(), "--defaults"],
    )
    .success()
    .silent_about("trust");
}
