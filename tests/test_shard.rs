//! `git tpl test --shard` / `--record-durations` — ADR-036.
//!
//! Real Git throughout, on the same principle as the rest of the suite: a
//! mock would only prove the mock behaves.

mod common;

use std::path::Path;

use common::{Template, tpl};

/// A template with one question and however many cases the test needs, each
/// asserting only `[answers]` — the split under test is which cases run,
/// not what any one of them checks.
fn template(parent: &Path, cases: &[(&str, &str)]) -> Template {
    let built = Template::minimal(
        parent,
        "name = \"shardable\"\n",
        &[("marker.txt", "hello\n")],
    );
    for (path, body) in cases {
        built.repo.write(path, body);
    }
    if !cases.is_empty() {
        built.repo.commit_all("test: cases");
    }
    built
}

/// Run `git tpl test --json` from inside the template's own repository.
fn run(template: &Template, args: &[&str]) -> common::Output {
    let mut all = vec!["--json", "test"];
    all.extend_from_slice(args);
    tpl(&template.repo, &all)
}

/// The same, without `--json` — for asserting on the human-readable report.
fn run_human(template: &Template, args: &[&str]) -> common::Output {
    let mut all = vec!["test"];
    all.extend_from_slice(args);
    tpl(&template.repo, &all)
}

fn case_names(output: &common::Output) -> Vec<String> {
    output.json()["cases"]
        .as_array()
        .expect("cases array")
        .iter()
        .map(|case| case["name"].as_str().expect("name").to_string())
        .collect()
}

/// Four trivial cases, named so alphabetical (= discovery) order is `a b c d`.
fn four_cases(parent: &Path) -> Template {
    template(
        parent,
        &[
            ("tests/a.toml", "[answers]\n"),
            ("tests/b.toml", "[answers]\n"),
            ("tests/c.toml", "[answers]\n"),
            ("tests/d.toml", "[answers]\n"),
        ],
    )
}

fn write_durations(template: &Template, lines: &str) {
    template.repo.write(
        "tests/__timings__/durations",
        &format!("# git-tpl durations 1\n{lines}"),
    );
}

// --- even split, no durations file ------------------------------------------

#[test]
fn an_even_split_with_no_durations_file_divides_by_count() {
    let dir = tempfile::tempdir().unwrap();
    let template = four_cases(dir.path());

    let first = case_names(&run(&template, &["--shard", "1/2"]).success());
    let second = case_names(&run(&template, &["--shard", "2/2"]).success());

    assert_eq!(first, vec!["a", "b"], "{first:?}");
    assert_eq!(second, vec!["c", "d"], "{second:?}");
}

#[test]
fn shard_total_of_one_runs_everything() {
    let dir = tempfile::tempdir().unwrap();
    let template = four_cases(dir.path());

    let names = case_names(&run(&template, &["--shard", "1/1"]).success());
    assert_eq!(names, vec!["a", "b", "c", "d"]);
}

#[test]
fn the_shard_summary_is_null_on_a_plain_run_and_populated_under_shard() {
    let dir = tempfile::tempdir().unwrap();
    let template = four_cases(dir.path());

    let plain = run(&template, &[]).success();
    assert!(plain.json()["shard"].is_null(), "{}", plain.json());

    let sharded = run(&template, &["--shard", "1/2"]).success();
    let shard = &sharded.json()["shard"];
    assert_eq!(shard["index"], 1);
    assert_eq!(shard["total"], 2);
    assert_eq!(shard["casesTotal"], 4);
    assert_eq!(shard["balanced"], false);
}

#[test]
fn the_human_output_names_the_shard_and_whether_it_was_balanced() {
    let dir = tempfile::tempdir().unwrap();
    let template = four_cases(dir.path());

    run_human(&template, &[]).success().silent_about("Shard:");

    run_human(&template, &["--shard", "1/2"])
        .success()
        .says("Shard:")
        .says("1/2")
        .says("split evenly by count");

    write_durations(&template, "a 100\nb 1\nc 1\nd 1\n");
    run_human(&template, &["--shard", "1/2"])
        .success()
        .says("balanced by recorded duration");
}

#[test]
fn every_case_reports_a_duration_in_milliseconds_even_without_shard_or_record_durations() {
    let dir = tempfile::tempdir().unwrap();
    let template = four_cases(dir.path());

    let output = run(&template, &[]).success();
    for case in output.json()["cases"].as_array().unwrap() {
        assert!(case["durationMs"].is_u64(), "{case}");
    }
}

// --- balanced split, with a durations file ----------------------------------

#[test]
fn a_shard_with_a_durations_file_balances_by_recorded_time() {
    let dir = tempfile::tempdir().unwrap();
    let template = four_cases(dir.path());
    // `a` is a hundred times slower than the rest: an even-by-count split
    // would put it beside one other case; a balanced split puts it alone.
    write_durations(&template, "a 100\nb 1\nc 1\nd 1\n");

    let first = case_names(&run(&template, &["--shard", "1/2"]).success());
    let second = case_names(&run(&template, &["--shard", "2/2"]).success());

    assert_eq!(first, vec!["a"], "{first:?}");
    assert_eq!(second, vec!["b", "c", "d"], "{second:?}");

    let shard = run(&template, &["--shard", "1/2"]).success().json()["shard"].clone();
    assert_eq!(shard["balanced"], true, "{shard}");
}

#[test]
fn an_unrecorded_case_is_weighted_at_the_mean_of_recorded_ones() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[
            ("tests/a.toml", "[answers]\n"),
            ("tests/b.toml", "[answers]\n"),
            ("tests/c.toml", "[answers]\n"),
        ],
    );
    // Only `a` is recorded. If an unrecorded case were weighted as zero
    // (rather than the mean of what is known), `b` and `c` would both land
    // in whichever bucket is emptiest first and the split below would come
    // out differently — see the by-hand trace in ADR-036's PR description.
    write_durations(&template, "a 90\n");

    let first = case_names(&run(&template, &["--shard", "1/2"]).success());
    let second = case_names(&run(&template, &["--shard", "2/2"]).success());

    assert_eq!(first, vec!["a", "c"], "{first:?}");
    assert_eq!(second, vec!["b"], "{second:?}");
}

// --- malformed / out-of-range shards -----------------------------------------

#[test]
fn a_shard_index_exceeding_total_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let template = four_cases(dir.path());

    let output = run(&template, &["--shard", "5/4"]).failure();
    assert_eq!(output.error_code(), "tpl::testing::malformed_shard");
}

#[test]
fn a_shard_spec_that_is_not_index_slash_total_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let template = four_cases(dir.path());

    for spec in ["nonsense", "2", "0/4", "1/0"] {
        let output = run(&template, &["--shard", spec]).failure();
        assert_eq!(
            output.error_code(),
            "tpl::testing::malformed_shard",
            "spec {spec:?}"
        );
    }
}

#[test]
fn a_shard_with_more_shards_than_cases_is_refused_rather_than_silently_empty() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        &[
            ("tests/a.toml", "[answers]\n"),
            ("tests/b.toml", "[answers]\n"),
        ],
    );

    let output = run(&template, &["--shard", "3/3"]).failure();
    assert_eq!(output.error_code(), "tpl::testing::empty_shard");
    assert!(output.all().contains('2'), "{}", output.all());
}

// --- --record-durations ------------------------------------------------------

#[test]
fn record_durations_writes_a_durations_file_with_every_cases_measured_time() {
    let dir = tempfile::tempdir().unwrap();
    let template = four_cases(dir.path());

    run(&template, &["--record-durations"]).success();

    let recorded = template.repo.read("tests/__timings__/durations");
    assert!(
        recorded.starts_with("# git-tpl durations 1\n"),
        "{recorded}"
    );
    for name in ["a", "b", "c", "d"] {
        assert!(recorded.contains(name), "{recorded}");
    }
    // Sorted by name, so a re-recording never produces a spurious diff for a
    // reviewer to squint at.
    let names: Vec<&str> = recorded
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| line.split(' ').next().unwrap())
        .collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted, "{recorded}");
}

#[test]
fn record_durations_merges_rather_than_replaces_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let template = four_cases(dir.path());
    write_durations(&template, "a 1\nb 2\nc 3\nd 4\n");

    // Filtered to one case: the other three must survive untouched.
    run(&template, &["--record-durations", "a"]).success();

    let recorded = template.repo.read("tests/__timings__/durations");
    assert!(recorded.contains("b 2"), "{recorded}");
    assert!(recorded.contains("c 3"), "{recorded}");
    assert!(recorded.contains("d 4"), "{recorded}");
    // `a`'s own line changed to whatever this run actually measured, so only
    // assert it is present at all, not with its old value.
    assert!(
        recorded.lines().any(|line| line.starts_with("a ")),
        "{recorded}"
    );
}

#[test]
fn record_durations_conflicts_with_shard_and_write_at_the_cli_layer() {
    let dir = tempfile::tempdir().unwrap();
    let template = four_cases(dir.path());

    run(&template, &["--record-durations", "--shard", "1/2"])
        .code(2)
        .says("cannot be used with");
    run(&template, &["--record-durations", "--write"])
        .code(2)
        .says("cannot be used with");
}

#[cfg(unix)]
#[test]
fn record_durations_that_cannot_be_written_names_the_path_and_the_reason() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let template = four_cases(dir.path());

    let tests_dir = template.repo.path.join("tests");
    let original = std::fs::metadata(&tests_dir).unwrap().permissions();
    let mut locked = original.clone();
    locked.set_mode(0o500);
    std::fs::set_permissions(&tests_dir, locked).unwrap();

    let output = run(&template, &["--record-durations"]).failure();

    // Restore before asserting, or a failure here leaves an undeletable
    // temporary directory behind.
    std::fs::set_permissions(&tests_dir, original).unwrap();

    assert_eq!(output.error_code(), "tpl::testing::durations_write");
}

// --- malformed durations file -------------------------------------------------

#[test]
fn an_unreadable_durations_entry_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let template = four_cases(dir.path());
    // No space at all: `parse_durations` cannot split a name from a
    // millisecond count.
    write_durations(&template, "onlyname\n");

    let output = run(&template, &["--shard", "1/2"]).failure();
    assert_eq!(output.error_code(), "tpl::testing::durations_read");
}

// --- durations read from a committed revision, via --ref --------------------

#[test]
fn a_shard_with_ref_reads_the_durations_committed_at_that_revision() {
    let dir = tempfile::tempdir().unwrap();
    let template = four_cases(dir.path());
    template.repo.write(
        "tests/__timings__/durations",
        "# git-tpl durations 1\na 100\nb 1\nc 1\nd 1\n",
    );
    template.repo.commit_all("test: record durations");
    template.repo.git(&["tag", "v1"]);

    let first = case_names(&run(&template, &["--ref", "v1", "--shard", "1/2"]).success());
    let second = case_names(&run(&template, &["--ref", "v1", "--shard", "2/2"]).success());

    // Same balanced split `a_shard_with_a_durations_file_balances_by_recorded_time`
    // asserts for the dirty (default) read — proving the committed-revision
    // path reads the same file, not merely that some file was read.
    assert_eq!(first, vec!["a"], "{first:?}");
    assert_eq!(second, vec!["b", "c", "d"], "{second:?}");
}

#[test]
fn a_shard_with_ref_and_no_durations_falls_back_to_even_split() {
    let dir = tempfile::tempdir().unwrap();
    let template = four_cases(dir.path());
    template.repo.git(&["tag", "v1"]);

    let output = run(&template, &["--ref", "v1", "--shard", "1/2"]).success();
    assert_eq!(
        output.json()["shard"]["balanced"],
        false,
        "{}",
        output.json()
    );
    let first = case_names(&output);
    assert_eq!(first, vec!["a", "b"], "{first:?}");
}

// --- a durations file that names none of the current cases -------------------

#[test]
fn a_durations_file_matching_no_current_case_falls_back_to_equal_zero_weight() {
    let dir = tempfile::tempdir().unwrap();
    let template = four_cases(dir.path());
    // Every recorded name is stale — none of `a`, `b`, `c`, `d` appears — so
    // the mean of "known" durations is the empty-set fallback, `0`, and
    // every current case is weighted identically. The tie-break then packs
    // every case into the lightest (lowest-index) bucket first, so the
    // second shard gets nothing: a durations file that has drifted this far
    // from the current suite is not silently balanced, it is loudly empty.
    write_durations(&template, "renamed_long_ago 500\n");

    let first = case_names(&run(&template, &["--shard", "1/2"]).success());
    assert_eq!(first, vec!["a", "b", "c", "d"], "{first:?}");

    let output = run(&template, &["--shard", "2/2"]).failure();
    assert_eq!(output.error_code(), "tpl::testing::empty_shard");
}
