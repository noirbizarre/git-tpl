//! Persistent source objects and network policy, exercised against real Git.
mod common;

use common::{Repo, file_url, tpl};
use std::time::Duration;
use tpl::git::cache::{Access, CacheError, identity, redact};
use tpl::git::{GitBackend, Oid};

fn source() -> Repo {
    let repo = Repo::init("source");
    repo.write("template.toml", "name = 'cached'\n");
    repo.write("template/value", "first\n");
    repo.commit_all("initial source");
    repo
}

fn access(root: &std::path::Path, offline: bool) -> Access {
    Access::new(Some(root.to_owned()), offline)
}

#[test]
fn a_cached_full_commit_is_reusable_after_the_origin_disappears() {
    let source = source();
    let url = file_url(&source.path);
    let revision = source.rev_parse("HEAD");
    let root = tempfile::tempdir().unwrap();
    let first = access(root.path(), false)
        .acquire(&url, Some(&revision), false)
        .unwrap();
    let expected = first.repo.commit_tree(first.revision).unwrap();
    drop(first);
    std::fs::remove_dir_all(&source.path).unwrap();
    for offline in [false, true] {
        let cached = access(root.path(), offline)
            .acquire(&url, Some(&revision), false)
            .unwrap();
        assert_eq!(cached.repo.commit_tree(cached.revision).unwrap(), expected);
    }
    assert!(
        access(root.path(), false)
            .acquire(&url, Some("main"), false)
            .is_err()
    );
}

#[test]
fn offline_misses_and_symbolic_refs_have_distinct_actionable_diagnostics() {
    let root = tempfile::tempdir().unwrap();
    let cache = access(root.path(), true);
    assert!(matches!(
        cache.acquire("https://example.invalid/t", Some("main"), false),
        Err(CacheError::OfflineReference { .. })
    ));
    assert!(matches!(
        cache.acquire("https://example.invalid/t", Some(&"a".repeat(40)), false),
        Err(CacheError::OfflineMiss { .. })
    ));
}

#[test]
fn source_fetch_failures_distinguish_network_from_local_cache_storage() {
    let root = tempfile::tempdir().unwrap();
    let cache = access(root.path(), false);
    let absent = file_url(&root.path().join("absent"));
    assert!(matches!(
        cache.acquire(&absent, None, false),
        Err(CacheError::Git(tpl::git::GitError::Network { .. }))
    ));
    let source = source();
    let blocked = root.path().join("blocked");
    std::fs::write(&blocked, b"not a directory").unwrap();
    assert!(matches!(
        access(&blocked, false).acquire(&file_url(&source.path), None, false),
        Err(CacheError::Io { .. })
    ));
}

#[test]
fn full_commit_requests_cannot_be_redirected_by_hexadecimal_tags() {
    let source = source();
    let sha = source.rev_parse("HEAD");
    source.write("template/value", "different\n");
    source.commit_all("different tag target");
    source.git(&["tag", &sha]);
    let root = tempfile::tempdir().unwrap();
    let url = file_url(&source.path);
    for pinned in [false, true] {
        let resolved = access(root.path(), false)
            .acquire(&url, Some(&sha), pinned)
            .unwrap();
        assert_eq!(resolved.revision.to_hex(), sha);
    }
    let entry = root.path().join("v1/repos").join(identity(&url));
    let fetch_head = std::fs::read_to_string(entry.join("FETCH_HEAD")).unwrap_or_default();
    assert!(
        !fetch_head.contains(&url),
        "requested URLs must not persist through fetch metadata: {fetch_head}"
    );
    let config = std::fs::read_to_string(entry.join("config")).unwrap();
    assert!(
        !config.contains(&url),
        "anonymous remotes must not persist their URLs"
    );
}

#[test]
fn one_remote_can_supply_multiple_ancestor_layers_without_locking_itself_out() {
    let source = source();
    let url = file_url(&source.path);
    let base = source.rev_parse("HEAD");
    source.write(
        "template.toml",
        &format!("name = 'child'\n[extends]\nsource = '{url}'\nrev = '{base}'\n"),
    );
    source.commit_all("extend earlier commit in same source");
    let consumer = Repo::init("consumer");
    let output = consumer.path.join("out");
    tpl(
        &consumer,
        &[
            "render",
            &url,
            "--trust",
            "--defaults",
            "-o",
            output.to_str().unwrap(),
        ],
    )
    .success();
    assert_eq!(consumer.read("out/value"), "first\n");
}

#[test]
fn new_invocations_refresh_moved_tags_branches_and_default_head() {
    let source = source();
    source.git(&["tag", "v1"]);
    let url = file_url(&source.path);
    let root = tempfile::tempdir().unwrap();
    let first_access = access(root.path(), false);
    let old = first_access
        .acquire(&url, Some("v1"), true)
        .unwrap()
        .revision;
    source.write("template/value", "second\n");
    source.commit_all("move source");
    source.git(&["tag", "-f", "v1"]);
    source.git(&["branch", "alternate"]);
    source.git(&["symbolic-ref", "HEAD", "refs/heads/alternate"]);
    let next = access(root.path(), false);
    let tagged = next.acquire(&url, Some("v1"), true).unwrap();
    assert_ne!(tagged.revision, old);
    assert_eq!(
        first_access
            .acquire(&url, Some("v1"), true)
            .unwrap()
            .revision,
        old,
        "one invocation keeps its resolved object even after another process refreshes refs"
    );
    let default = next.acquire(&url, None, false).unwrap();
    assert_eq!(default.reference, "alternate");
    assert_eq!(default.revision, tagged.revision);
    assert_eq!(
        next.acquire(&url, Some("main"), false).unwrap().revision,
        tagged.revision
    );
}

#[test]
fn deleted_refs_never_resolve_from_stale_cached_names() {
    let source = source();
    source.git(&["tag", "gone"]);
    source.git(&["branch", "old"]);
    let url = file_url(&source.path);
    let root = tempfile::tempdir().unwrap();
    access(root.path(), false)
        .acquire(&url, Some("gone"), false)
        .unwrap();
    source.git(&["tag", "-d", "gone"]);
    source.git(&["branch", "-D", "old"]);
    let next = access(root.path(), false);
    assert!(next.acquire(&url, Some("gone"), false).is_err());
    assert!(next.acquire(&url, Some("old"), false).is_err());
}

#[test]
fn clear_and_prune_preserve_live_readers_and_release_on_drop() {
    let source = source();
    let root = tempfile::tempdir().unwrap();
    let cache = access(root.path(), false);
    let url = file_url(&source.path);
    let reader = cache.acquire(&url, None, false).unwrap();
    let id = identity(&url);
    assert_eq!(cache.clear(None, None).unwrap().skipped, vec![id.clone()]);
    let output = tpl(
        &source,
        &[
            "--cache-dir",
            root.path().to_str().unwrap(),
            "--json",
            "cache",
            "clear",
        ],
    )
    .success()
    .json();
    assert_eq!(output["skipped"], serde_json::json!([id]));
    reader.repo.commit_tree(reader.revision).unwrap();
    drop(reader);
    assert!(
        cache
            .clear(None, Some(Duration::from_secs(86400)))
            .unwrap()
            .removed
            .is_empty()
    );
    assert_eq!(
        cache
            .clear(None, Some(Duration::ZERO))
            .unwrap()
            .removed
            .len(),
        1
    );
    assert!(cache.list().unwrap().is_empty());
}

#[test]
fn cache_management_works_outside_a_project_and_reports_json() {
    let source = source();
    let url = file_url(&source.path);
    let dir = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| common::tpl_outside(dir.path(), config.path(), args);
    let empty = run(&["--json", "cache", "list"]).success().json();
    assert_eq!(empty["entries"], serde_json::json!([]));
    run(&["cache", "fetch", &url]).success();
    let listed = run(&["--json", "cache", "list"]).success().json();
    assert_eq!(listed["entries"].as_array().unwrap().len(), 1);
    assert!(listed["entries"][0]["bytes"].as_u64().unwrap() > 0);
    run(&["cache", "prune", "--older-than", "30"]).success();
    let removed = run(&["--json", "cache", "clear", &url]).success().json();
    assert_eq!(removed["removed"].as_array().unwrap().len(), 1);
}

#[test]
fn independent_processes_can_initialize_and_refresh_one_entry_concurrently() {
    let source = source();
    let url = file_url(&source.path);
    let handles: Vec<_> = (0..6)
        .map(|_| {
            let path = source.path.clone();
            let config = source.config_home().to_owned();
            let url = url.clone();
            std::thread::spawn(move || {
                common::tpl_outside(&path, &config, &["cache", "fetch", &url]).success()
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let listed = tpl(&source, &["--json", "cache", "list"]).success().json();
    assert_eq!(listed["entries"].as_array().unwrap().len(), 1);
}

#[test]
fn cache_identity_keeps_transports_accounts_ports_and_paths_distinct() {
    let sources = [
        "https://example.org/Org/repo",
        "https://example.org/org/repo",
        "https://example.org/Org/repo.git",
        "https://example.org/Org/repo/",
        "ssh://git@example.org/Org/repo",
        "git@example.org:Org/repo",
        "ssh://other@example.org/Org/repo",
        "ssh://git@example.org:2222/Org/repo",
        "ssh://git@[::1]:2222/Org/repo",
        "file:///tmp/Org/repo",
        "https://user:secret@example.org/Org/repo",
    ];
    let ids: std::collections::BTreeSet<_> =
        sources.iter().map(|source| identity(source)).collect();
    assert_eq!(ids.len(), sources.len());
    assert_eq!(
        redact("https://user:secret@example.org/Org/repo?token=secret#fragment"),
        "https://example.org/Org/repo"
    );
}

#[test]
fn a_partial_commit_tree_is_not_an_offline_hit() {
    let source = source();
    let root = tempfile::tempdir().unwrap();
    let url = file_url(&source.path);
    let sha = source.rev_parse("HEAD");
    let cache = access(root.path(), false);
    let reader = cache.acquire(&url, Some(&sha), false).unwrap();
    let tree = reader.repo.commit_tree(reader.revision).unwrap();
    let blob = reader
        .repo
        .read_path(tree, "template/value")
        .unwrap()
        .unwrap();
    assert_eq!(blob, b"first\n");
    drop(reader);
    let entry = root.path().join("v1/repos").join(identity(&url));
    // Unpack before removing a blob so the commit itself remains a misleading possible cache hit.
    let cached = Repo::at(entry.clone());
    let blob_id = cached.git(&["rev-parse", &format!("{sha}:template/value")]);
    let pack = std::fs::read_dir(entry.join("objects/pack"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "pack"))
        .unwrap();
    let bytes = std::fs::read(pack).unwrap();
    std::fs::remove_dir_all(entry.join("objects/pack")).unwrap();
    use std::io::Write;
    let mut command = std::process::Command::new("git");
    common::scrub_git_env(&mut command, cached.config_home());
    let mut child = command
        .current_dir(&entry)
        .arg("unpack-objects")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&bytes).unwrap();
    assert!(child.wait().unwrap().success());
    let oid = Oid::parse(sha.trim()).unwrap();
    assert!(
        tpl::git::LibGit2::open(&entry)
            .unwrap()
            .complete_commit(oid)
            .unwrap()
    );
    let blob_id = blob_id.trim();
    std::fs::remove_file(
        entry
            .join("objects")
            .join(&blob_id[..2])
            .join(&blob_id[2..]),
    )
    .unwrap();
    assert!(
        tpl::git::LibGit2::open(&entry)
            .unwrap()
            .commit_tree(oid)
            .is_ok()
    );
    assert!(
        access(root.path(), true)
            .acquire(&url, Some(&sha), false)
            .is_err()
    );
}

#[test]
fn offline_render_blocks_http_and_rejects_publication_before_mutation() {
    let source = source();
    source.write(
        "template.toml",
        "name = 'cached'\n[data.remote]\nsource = 'http://127.0.0.1:1/never'\nformat = 'toml'\n",
    );
    source.write("template/value.jinja", "{{ data.remote.value }}");
    source.commit_all("declare remote data");
    let output = source.path.join("out");
    let result = tpl(
        &source,
        &[
            "--json",
            "--offline",
            "render",
            source.path.to_str().unwrap(),
            "--trust",
            "--defaults",
            "-o",
            output.to_str().unwrap(),
        ],
    )
    .failure();
    assert_eq!(result.error_code(), "tpl::data::offline");
    assert!(!output.exists());
    for command in ["fetch", "push"] {
        let state = source.working_state();
        assert_eq!(
            tpl(&source, &["--json", "--offline", command])
                .failure()
                .error_code(),
            "tpl::cache::offline"
        );
        assert_eq!(source.working_state(), state);
    }
}

#[test]
fn cached_ancestors_and_git_data_render_offline_without_bypassing_consent() {
    let base = source();
    let data = Repo::init("data");
    data.write("values.toml", "value = 'cached data'\n");
    data.commit_all("initial data");
    let leaf = source();
    leaf.write("template.toml", &format!(
        "name = 'leaf'\n[extends]\nsource = '{}'\nrev = '{}'\n[data.remote]\nsource = '{}'\nref = '{}'\npath = 'values.toml'\n",
        file_url(&base.path), base.rev_parse("HEAD"), file_url(&data.path), data.rev_parse("HEAD")));
    leaf.write("template/data.jinja", "{{ data.remote.value }}");
    leaf.commit_all("add ancestor and data");
    let url = file_url(&leaf.path);
    let sha = leaf.rev_parse("HEAD");
    let project = Repo::init("consumer");
    let output = project.path.join("out");
    let output = output.to_str().unwrap();
    tpl(
        &project,
        &[
            "render",
            &url,
            "--ref",
            &sha,
            "--trust",
            "--defaults",
            "-o",
            output,
        ],
    )
    .success();
    std::fs::remove_dir_all(&base.path).unwrap();
    std::fs::remove_dir_all(&data.path).unwrap();
    std::fs::remove_dir_all(&leaf.path).unwrap();
    let refused = tpl(
        &project,
        &[
            "--json",
            "--offline",
            "render",
            &url,
            "--ref",
            &sha,
            "--defaults",
            "-o",
            output,
            "--force",
        ],
    )
    .failure();
    assert_eq!(refused.error_code(), "tpl::extends::untrusted");
    tpl(
        &project,
        &[
            "--offline",
            "render",
            &url,
            "--ref",
            &sha,
            "--trust",
            "--defaults",
            "-o",
            output,
            "--force",
        ],
    )
    .success();
    assert_eq!(project.read("out/data"), "cached data");
}

#[test]
fn offline_update_refuses_automatic_push_before_advancing_the_rendered_ref() {
    let world = common::World::new();
    world.init(&[]).success();
    world.move_template();
    world.project.git(&["config", "tpl.autoPush", "true"]);
    let before = world.project.working_state();
    let tip = world.project.rev_parse(&world.ref_name());
    assert_eq!(
        tpl(
            &world.project,
            &["--json", "--offline", "update", "--defaults"]
        )
        .failure()
        .error_code(),
        "tpl::cache::offline"
    );
    assert_eq!(world.project.working_state(), before);
    assert_eq!(world.project.rev_parse(&world.ref_name()), tip);
    world.project.git(&["config", "tpl.autoPush", "false"]);
    tpl(&world.project, &["--offline", "update", "--defaults"]).success();
}

#[test]
fn abandoned_staging_is_recovered_and_corrupt_metadata_can_be_cleared() {
    let source = source();
    let root = tempfile::tempdir().unwrap();
    let url = file_url(&source.path);
    let id = identity(&url);
    let abandoned = root
        .path()
        .join("v1/repos")
        .join(format!(".staging-{id}-abandoned"));
    std::fs::create_dir_all(&abandoned).unwrap();
    std::fs::write(abandoned.join("partial"), b"interrupted transfer").unwrap();
    let cache = access(root.path(), false);
    drop(cache.acquire(&url, None, false).unwrap());
    assert!(!abandoned.exists());
    std::fs::write(
        root.path()
            .join("v1/repos")
            .join(&id)
            .join("git-tpl-cache.json"),
        b"invalid",
    )
    .unwrap();
    assert!(matches!(cache.list(), Err(CacheError::Corrupt { .. })));
    let result = cache.clear(Some(&url), None).unwrap();
    assert_eq!(result.removed, vec![id], "{result:?}");
}

/// Reproducible local-transfer baseline without a benchmark dependency or timing thresholds.
#[test]
#[ignore = "manual resolution-cost measurement; not a timing assertion"]
fn measure_source_resolution_costs() {
    use std::time::Instant;
    let source = source();
    for version in 0..40 {
        let body: String = (0..4000)
            .map(|line| format!("revision {version}: source line {line}\n"))
            .collect();
        source.write("template/history", &body);
        source.commit_all(&format!("history {version}"));
    }
    let url = file_url(&source.path);
    let sha = source.rev_parse("HEAD");
    let root = tempfile::tempdir().unwrap();
    let measure = |label: &str, run: &mut dyn FnMut()| {
        let start = Instant::now();
        for _ in 0..10 {
            run();
        }
        println!(
            "{label}: {:.2} ms/invocation",
            start.elapsed().as_secs_f64() * 100.0
        );
    };
    measure("fresh bare clone baseline", &mut || {
        let dir = tempfile::tempdir().unwrap();
        let repo = tpl::git::LibGit2::clone_bare(&url, dir.path()).unwrap();
        repo.resolve_revision(&sha, &url).unwrap();
    });
    measure("cold cache acquisition", &mut || {
        let dir = tempfile::tempdir().unwrap();
        access(dir.path(), false)
            .acquire(&url, Some(&sha), false)
            .unwrap();
    });
    access(root.path(), false).fetch(&url).unwrap();
    measure("warm full commit", &mut || {
        access(root.path(), false)
            .acquire(&url, Some(&sha), false)
            .unwrap();
    });
    measure("warm branch refresh", &mut || {
        access(root.path(), false)
            .acquire(&url, Some("main"), false)
            .unwrap();
    });
    measure("four repeated source/ref acquisitions", &mut || {
        let access = access(root.path(), false);
        let handles: Vec<_> = (0..4)
            .map(|_| access.acquire(&url, Some("main"), false).unwrap())
            .collect();
        drop(handles);
    });
    source.write("template/incremental", "new data\n");
    source.commit_all("incremental update");
    let start = Instant::now();
    access(root.path(), false)
        .acquire(&url, Some("main"), false)
        .unwrap();
    println!(
        "incremental refresh: {:.2} ms",
        start.elapsed().as_secs_f64() * 1000.0
    );
    println!(
        "source cache bytes: {}",
        access(root.path(), false).list().unwrap()[0].bytes
    );
}
