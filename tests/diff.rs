//! `git tpl diff --stat` and `--name-only`.
//!
//! The patch mode and the merge matrix live in `tests/merge.rs`; this file is
//! about the summary modes, whose numbers must agree with `git diff --stat`'s.

mod common;

use common::{World, tpl};

/// A world whose template has moved and been rendered, so a diff has something
/// to report: `README.md` gains two lines, `release.yml` is new.
fn pending() -> World {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();
    world
}

/// The line of `--stat` output about `path`, whitespace collapsed.
fn stat_line(output: &str, path: &str) -> String {
    let line = output
        .lines()
        .find(|l| l.split_whitespace().nth(1) == Some(path))
        .unwrap_or_else(|| panic!("no line about {path} in:\n{output}"));
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn stat_counts_the_lines_a_merge_would_insert() {
    let world = pending();

    let output = tpl(&world.project, &["diff", "--stat"]).success();

    // The template appended a blank line and a sentence to `README.md`, and
    // added a one-line workflow.
    assert_eq!(
        stat_line(&output.all(), "README.md"),
        "modified README.md +2 -0"
    );
    assert_eq!(
        stat_line(&output.all(), ".github/workflows/release.yml"),
        "added .github/workflows/release.yml +1 -0"
    );
}

#[test]
fn stat_reports_a_deleted_file_as_deletions_only() {
    let world = pending();

    let output = tpl(&world.project, &["diff", "--stat"]).success();

    // The project's own file: the template does not produce it, so a tree diff
    // reports it as a deletion. Documented, and the count must reflect it.
    assert_eq!(
        stat_line(&output.all(), "NOTES.md"),
        "deleted NOTES.md +0 -1"
    );
}

#[test]
fn stat_summarises_the_totals_the_way_git_does() {
    let world = pending();

    tpl(&world.project, &["diff", "--stat"])
        .success()
        .says("files changed,")
        .says("insertions(+)")
        .says("deletions(-)");
}

#[test]
fn stat_totals_agree_with_git_diff_stat() {
    let world = pending();

    let ours = tpl(&world.project, &["diff", "--stat"]).success();
    let summary = ours
        .all()
        .lines()
        .find(|l| l.contains("changed,"))
        .expect("a summary line")
        .trim()
        .to_string();

    // The premise of the project is that Git's behaviour is the behaviour, so
    // the numbers are checked against Git's own rather than against a literal
    // somebody would have to recompute by hand.
    let theirs = world
        .project
        .git(&["diff", "--stat", "HEAD", &world.ref_name()]);
    let git_summary = theirs.lines().next_back().expect("a summary line").trim();

    assert_eq!(summary, git_summary);
}

#[test]
fn stat_can_be_limited_to_paths() {
    let world = pending();

    let output = tpl(&world.project, &["diff", "--stat", "--", "README.md"]).success();

    assert!(output.all().contains("README.md"), "{}", output.all());
    assert!(!output.all().contains("release.yml"), "{}", output.all());
    // One path, so Git's singular.
    assert!(output.all().contains("1 file changed"), "{}", output.all());
}

#[test]
fn stat_reversed_swaps_insertions_and_deletions() {
    let world = pending();

    let forward = tpl(&world.project, &["diff", "--stat"]).success();
    let reversed = tpl(&world.project, &["diff", "--stat", "--reverse"]).success();

    assert_eq!(
        stat_line(&forward.all(), "README.md"),
        "modified README.md +2 -0"
    );
    assert_eq!(
        stat_line(&reversed.all(), "README.md"),
        "modified README.md +0 -2"
    );
}

#[test]
fn stat_reports_a_binary_file_without_line_counts() {
    let world = World::new();
    world.init(&[]).success();

    // A PNG header: NUL bytes in the first block, which is what makes it
    // binary to Git and to us.
    let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR\x00\x00";
    std::fs::write(world.template.repo.path.join("template/logo.png"), png).expect("write png");
    world.template.repo.commit_all("feat: add a logo");
    tpl(&world.project, &["update", "--defaults"]).success();

    let output = tpl(&world.project, &["diff", "--stat"]).success();

    assert_eq!(stat_line(&output.all(), "logo.png"), "added logo.png Bin");
}

#[test]
fn stat_says_nothing_when_the_rendering_is_merged() {
    let world = World::new();
    world.init(&[]).success();

    // Only the template's own files can be asserted on: a tree diff also
    // reports every file the project has and the template does not.
    let output = tpl(&world.project, &["diff", "--stat", "--", "Cargo.toml"]).success();

    assert!(output.all().contains("No differences."), "{}", output.all());
}

#[test]
fn name_only_can_be_limited_to_paths() {
    let world = pending();

    let output = tpl(&world.project, &["diff", "--name-only", "--", "README.md"]).success();

    assert!(output.stdout.contains("README.md"), "{}", output.stdout);
    assert!(!output.stdout.contains("release.yml"), "{}", output.stdout);
}

#[test]
fn name_only_wins_over_stat_so_a_pipe_stays_clean() {
    let world = pending();

    let output = tpl(&world.project, &["diff", "--name-only", "--stat"]).success();

    assert!(output.stdout.contains("README.md"), "{}", output.stdout);
    assert!(
        !output.all().contains("changed,"),
        "a summary reached a caller asking for paths:\n{}",
        output.all()
    );
}
