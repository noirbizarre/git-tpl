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
fn diff_does_not_report_project_only_files_as_deletions() {
    let world = pending();

    let output = tpl(&world.project, &["diff", "--name-only"]).success();

    // `NOTES.md` predates the template and `.config/git.tpl.toml` is git-tpl's
    // own; both are in the merge base, so merging deletes neither. A plain
    // `HEAD`-to-ref tree diff used to call them deletions, which buried the one
    // real change under every file the project owns.
    assert!(
        !output.stdout.contains("NOTES.md"),
        "a project-only file was reported:\n{}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("git.tpl.toml"),
        "git-tpl's own config was reported as changing:\n{}",
        output.stdout
    );
    assert!(output.stdout.contains("README.md"), "{}", output.stdout);
}

#[test]
fn diff_names_match_git_merge_tree() {
    let world = pending();

    let ours = tpl(&world.project, &["diff", "--name-only"]).success();

    // The plain-Git equivalent, spelled out: the tree a merge would produce,
    // diffed against HEAD. If these ever disagree, the command is lying about
    // what merging would do.
    let merged = world
        .project
        .git(&["merge-tree", "--write-tree", "HEAD", &world.ref_name()]);
    let theirs = world
        .project
        .git(&["diff", "--name-only", "HEAD", merged.trim()]);

    let mut mine: Vec<&str> = ours.stdout.lines().collect();
    let mut git: Vec<&str> = theirs.lines().filter(|l| !l.is_empty()).collect();
    mine.sort_unstable();
    git.sort_unstable();
    assert_eq!(mine, git);
}

#[test]
fn stat_reports_a_deleted_file_as_deletions_only() {
    // A file the template genuinely stopped producing. That is a deletion a
    // merge would really make, and it must still be reported as one.
    let world = World::new();
    world.init(&[]).success();
    world.template.repo.remove("template/ci.yml");
    world.template.repo.commit_all("feat: drop the CI workflow");
    tpl(&world.project, &["update", "--defaults"]).success();

    let output = tpl(&world.project, &["diff", "--stat"]).success();

    assert_eq!(stat_line(&output.all(), "ci.yml"), "deleted ci.yml +0 -5");
}

#[test]
fn stat_summarises_the_totals_the_way_git_does() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    // A removal as well, so the summary has all three terms to word.
    world.template.repo.remove("template/ci.yml");
    world.template.repo.commit_all("feat: drop the CI workflow");
    tpl(&world.project, &["update", "--defaults"]).success();

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
    let merged = world
        .project
        .git(&["merge-tree", "--write-tree", "HEAD", &world.ref_name()]);
    let theirs = world
        .project
        .git(&["diff", "--stat", "HEAD", merged.trim()]);
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

    // Nothing narrowed: a merged rendering changes nothing at all, project-only
    // files included.
    let output = tpl(&world.project, &["diff", "--stat"]).success();

    assert!(output.all().contains("No differences."), "{}", output.all());
}

#[test]
fn diff_touches_nothing() {
    let world = pending();

    let before = world.project.working_state();
    tpl(&world.project, &["diff"]).success();
    let after = world.project.working_state();

    // The preview merges in memory. If it ever merged for real, this is the
    // test that would say so.
    assert_eq!(before, after);
}

#[test]
fn a_conflicting_preview_warns_and_still_exits_zero() {
    let world = World::new();
    world.init(&[]).success();

    // Both sides change the same line of `README.md`: the project's edit and
    // the template's cannot both stand.
    world.project.write("README.md", "# Mine, entirely\n");
    world.project.commit_all("docs: rewrite the readme");
    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();

    let output = tpl(&world.project, &["diff"]).success();

    assert!(output.all().contains("would conflict"), "{}", output.all());
    assert!(output.all().contains("README.md"), "{}", output.all());
    // Markers, because that is what a merge would leave in the worktree.
    assert!(output.stdout.contains("<<<<<<<"), "{}", output.stdout);
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

/// `--no-merge` leaves the project and the rendering with no common ancestor.
/// Git merges unrelated histories by content, and so must the preview — this is
/// the "look before you merge" step `git tpl init --no-merge` exists for.
#[test]
fn diff_previews_a_merge_of_unrelated_histories() {
    let world = World::new();
    world.init(&["--no-merge"]).success();

    let output = tpl(&world.project, &["diff", "--name-only"]).success();

    assert!(output.stdout.contains("Cargo.toml"), "{}", output.stdout);
    assert!(
        !output.stdout.contains("NOTES.md"),
        "the project's own file is not a deletion:\n{}",
        output.stdout
    );
}

// ---------------------------------------------------------------------------
// `--json`: the same summary as data.
//
// It carries what the text modes cannot — the conflicts as an array rather
// than as chrome on stderr — and the numbers must be the same numbers, so the
// assertions below cross-check against `--stat`'s own output.
// ---------------------------------------------------------------------------

#[test]
fn diff_json_reports_the_stat_as_data() {
    let world = pending();

    let json = tpl(&world.project, &["--json", "diff"]).success().json();

    assert_eq!(json["ok"], true);
    let changes = json["changes"].as_array().expect("an array of changes");
    let find = |path: &str| {
        changes
            .iter()
            .find(|c| c["path"] == path)
            .unwrap_or_else(|| panic!("no change about {path} in {json}"))
    };

    // The same two changes `stat_counts_the_lines_a_merge_would_insert`
    // asserts in text, spelled as fields rather than as a formatted line.
    let readme = find("README.md");
    assert_eq!(readme["kind"], "modified");
    assert_eq!(readme["insertions"], 2);
    assert_eq!(readme["deletions"], 0);
    assert_eq!(readme["binary"], false);

    let workflow = find(".github/workflows/release.yml");
    assert_eq!(workflow["kind"], "added");
    assert_eq!(workflow["insertions"], 1);

    assert_eq!(json["insertions"], 3);
    assert_eq!(json["deletions"], 0);
    assert_eq!(json["conflicts"], serde_json::json!([]));
}

/// `deleted` is the third `ChangeKind`, and the one a caller is most likely to
/// branch on — nothing else in the payload says a merge would remove a file.
#[test]
fn diff_json_names_a_deletion_as_such() {
    let world = World::new();
    world.init(&[]).success();
    world.template.repo.remove("template/ci.yml");
    world.template.repo.commit_all("feat: drop the CI workflow");
    tpl(&world.project, &["update", "--defaults"]).success();

    let json = tpl(&world.project, &["--json", "diff"]).success().json();

    let deleted = json["changes"]
        .as_array()
        .expect("changes")
        .iter()
        .find(|c| c["path"] == "ci.yml")
        .expect("a change about ci.yml");
    assert_eq!(deleted["kind"], "deleted");
    assert_eq!(deleted["insertions"], 0);
    assert_eq!(deleted["deletions"], 5);
}

/// The single most valuable thing to know before merging, and the one the text
/// output relegates to stderr. The exit code stays zero: a conflicting preview
/// is a correct answer to the question asked.
#[test]
fn diff_json_names_the_files_that_would_conflict() {
    let world = World::new();
    world.init(&[]).success();

    let readme = world.project.read("README.md");
    world.project.write(
        "README.md",
        &format!("{readme}\nWritten by the user at the end.\n"),
    );
    world.project.commit_all("docs: append a line");

    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();

    let output = tpl(&world.project, &["--json", "diff"]).success();
    let json = output.json();

    assert_eq!(
        json["conflicts"],
        serde_json::json!(["README.md"]),
        "the conflicting path is data, not prose: {json}"
    );
    assert!(
        !output.stdout.contains("would conflict"),
        "the human warning leaked onto stdout: {}",
        output.stdout
    );
}
