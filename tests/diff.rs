//! `git tpl diff` presentation and structured output.
//!
//! Native Git owns patch/stat display; structured modes bypass its tools.
//! The broader merge matrix lives in `tests/merge.rs`.

mod common;

use common::{World, tpl};

#[test]
fn strict_previews_ignore_history_but_reject_current_unknown_inputs() {
    let world = World::new();
    world.init(&[]).success();
    let mut config = tpl::config::Config::load(&world.project.path).unwrap();
    config
        .answers
        .insert("obsolete".into(), tpl::Value::String("old".into()));
    config.save(&world.project.path).unwrap();
    let before = world.project.working_state();
    let tip = world.project.git(&["rev-parse", &world.ref_name()]);
    let config_before = world.project.read(".config/git.tpl.toml");
    let answers = world.dir.path().join("obsolete.toml");
    std::fs::write(&answers, "obsolete = 'explicit'\n").unwrap();
    for command in [vec!["--json", "diff"], vec!["--json", "show", "README.md"]] {
        let mut args = command.clone();
        args.extend(["--dirty", "--defaults", "--strict-answers"]);
        tpl(&world.project, &args).success();
        for extra in [
            vec!["--answer", "obsolete=explicit"],
            vec!["--answer", "project_nam=typo"],
            vec!["--answers-from", answers.to_str().unwrap()],
        ] {
            let mut supplied = args.clone();
            supplied.extend(extra);
            assert_eq!(
                tpl(&world.project, &supplied).failure().error_code(),
                "tpl::answers::unknown_key"
            );
        }
    }
    assert_eq!(world.project.working_state(), before);
    assert_eq!(world.project.git(&["rev-parse", &world.ref_name()]), tip);
    assert_eq!(world.project.read(".config/git.tpl.toml"), config_before);
}

#[test]
fn patch_and_stat_honor_git_color_and_explicit_overrides() {
    let world = pending();
    for args in [vec!["diff"], vec!["diff", "--stat"]] {
        let plain = common::tpl_auto(&world.project, &args).success();
        assert!(!plain.stdout.contains('\x1b'));
        world.project.git(&["config", "color.diff", "always"]);
        let configured = common::tpl_auto(&world.project, &args).success();
        assert!(configured.stdout.contains('\x1b'), "{}", configured.stdout);
        let disabled = tpl(&world.project, &args).success();
        assert!(!disabled.stdout.contains('\x1b'));
        world.project.git(&["config", "color.diff", "never"]);
        let forced = common::tpl_colored(&world.project, &args).success();
        assert!(forced.stdout.contains('\x1b'), "{}", forced.stdout);
    }
}

#[test]
fn stat_is_native_git_output_on_stdout() {
    let world = pending();
    let merged = world
        .project
        .git(&["merge-tree", "--write-tree", "HEAD", &world.ref_name()]);
    let expected = world
        .project
        .git(&["diff", "--color=never", "--stat", "HEAD", merged.trim()]);
    let output = tpl(&world.project, &["diff", "--stat"]).success();
    assert_eq!(output.stdout.trim(), expected);
    assert!(output.stderr.is_empty(), "{}", output.stderr);
}

#[test]
fn missing_native_git_is_actionable_and_does_not_affect_structured_modes() {
    let world = pending();
    let output = common::tpl_with_env(&world.project, &["diff"], &[("PATH", "")]).failure();
    output.says("tpl::ops::diff_display").says("PATH");
    common::tpl_with_env(&world.project, &["diff", "--stat"], &[("PATH", "")]).failure();
    common::tpl_with_env(&world.project, &["diff", "--name-only"], &[("PATH", "")])
        .success()
        .says("README.md");
    let json = common::tpl_with_env(&world.project, &["--json", "diff"], &[("PATH", "")])
        .success()
        .json();
    assert_eq!(json["ok"], true);
}

#[cfg(unix)]
#[test]
fn native_diff_uses_external_drivers_and_reports_their_failures() {
    let world = pending();
    world
        .project
        .write(".git/display-diff.sh", "printf 'external diff marker\\n'\n");
    let env = [("GIT_EXTERNAL_DIFF", "sh .git/display-diff.sh")];
    common::tpl_with_env(&world.project, &["diff", "--exit-code"], &env)
        .code(1)
        .says("external diff marker");
    common::tpl_with_env(&world.project, &["diff", "--name-only"], &env)
        .success()
        .silent_about("external diff marker");
    common::tpl_with_env(&world.project, &["--json", "diff"], &env)
        .success()
        .silent_about("external diff marker");

    world.project.write(".git/display-diff.sh", "exit 42\n");
    common::tpl_with_env(&world.project, &["diff"], &env)
        .failure()
        .says("tpl::ops::diff_display");
}

#[cfg(unix)]
#[test]
fn native_diff_uses_configured_text_conversion() {
    let world = pending();
    world
        .project
        .write(".git/info/attributes", "README.md diff=upper\n");
    world
        .project
        .git(&["config", "diff.upper.textconv", "tr a-z A-Z <"]);
    let output = tpl(&world.project, &["diff", "--", "README.md"]).success();
    assert!(
        output.stdout.contains("+GENERATED BY GIT-TPL."),
        "{}",
        output.stdout
    );
    assert!(!output.stdout.contains("+Generated by git-tpl."));
}

#[test]
fn native_diff_supports_an_unborn_head_without_updating_it() {
    let world = World::new();
    world.init(&["--no-merge"]).success();
    world
        .project
        .git(&["symbolic-ref", "HEAD", "refs/heads/unborn"]);
    let before = world.project.working_state();
    tpl(&world.project, &["diff", "--exit-code"])
        .code(1)
        .says("new file mode");
    tpl(&world.project, &["diff", "--reverse"])
        .success()
        .says("No differences.");
    assert_eq!(before, world.project.working_state());
}

#[test]
fn native_diff_keeps_root_relative_paths_from_a_subdirectory() {
    let world = pending();
    let nested = common::Repo::at(world.project.path.join(".config"));
    tpl(&nested, &["diff", "--", "README.md"])
        .success()
        .says("+Generated by git-tpl.")
        .silent_about("release.yml");
}

#[cfg(unix)]
mod terminal {
    use super::*;
    use std::io::Read;

    /// A captured pipe cannot exercise Git's automatic pager selection.
    fn diff(world: &World, args: &[&str], pager: Option<&str>) -> (u32, String) {
        let pair = portable_pty::native_pty_system()
            .openpty(portable_pty::PtySize {
                rows: 24,
                cols: 100,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("open terminal");
        let mut command =
            portable_pty::CommandBuilder::new(assert_cmd::cargo::cargo_bin!("git-tpl"));
        command.cwd(&world.project.path);
        // No ambient repository, pager, or user configuration may select the
        // developer's own tools in a test that is supposed to prove precedence.
        command.env_clear();
        command.env("PATH", std::env::var_os("PATH").expect("PATH"));
        command.env("HOME", world.project.config_home());
        command.env("XDG_CONFIG_HOME", world.project.config_home());
        command.env("GIT_CONFIG_NOSYSTEM", "1");
        command.env(
            "GIT_CONFIG_GLOBAL",
            world.project.config_home().join("absent.gitconfig"),
        );
        command.env("TERM", "xterm");
        if let Some(pager) = pager {
            command.env("GIT_PAGER", pager);
        }
        command.args(args);
        let mut reader = pair.master.try_clone_reader().expect("terminal reader");
        let mut child = pair.slave.spawn_command(command).expect("run git-tpl");
        // The parent's slave would keep the reader alive after the child exits.
        drop(pair.slave);
        let output = std::thread::spawn(move || {
            let mut output = String::new();
            reader.read_to_string(&mut output).expect("read terminal");
            output
        });
        let status = child.wait().expect("wait for git-tpl");
        (status.exit_code(), output.join().expect("read thread"))
    }

    #[test]
    fn patches_and_stats_use_git_pager_precedence_only_on_a_terminal() {
        let world = pending();
        world
            .project
            .git(&["config", "core.pager", "printf 'CORE_PAGER\\n'; cat"]);
        for args in [vec!["diff"], vec!["diff", "--stat"]] {
            let (code, output) = diff(&world, &args, None);
            assert_eq!(code, 0, "{output}");
            assert!(output.contains("CORE_PAGER"), "{output}");
        }
        world
            .project
            .git(&["config", "pager.diff", "printf 'DIFF_PAGER\\n'; cat"]);
        let (_, output) = diff(&world, &["diff"], None);
        assert!(output.contains("DIFF_PAGER"), "{output}");
        assert!(!output.contains("CORE_PAGER"), "{output}");
        let (_, output) = diff(&world, &["diff"], Some("printf 'ENV_PAGER\\n'; cat"));
        assert!(output.contains("ENV_PAGER"), "{output}");
        assert!(!output.contains("DIFF_PAGER"), "{output}");

        // The same configuration must not turn redirected output into pager output.
        tpl(&world.project, &["diff"])
            .success()
            .silent_about("DIFF_PAGER");
        world.project.git(&["config", "pager.diff", "false"]);
        let (_, output) = diff(&world, &["diff"], Some("printf 'ENV_PAGER\\n'; cat"));
        assert!(!output.contains("ENV_PAGER"), "{output}");
    }

    #[test]
    fn structured_modes_do_not_start_a_pager_even_on_a_terminal() {
        let world = pending();
        for args in [vec!["diff", "--name-only"], vec!["--json", "diff"]] {
            let (code, output) = diff(&world, &args, Some("printf 'UNWANTED_PAGER\\n'; cat"));
            assert_eq!(code, 0, "{output}");
            assert!(!output.contains("UNWANTED_PAGER"), "{output}");
            assert!(output.contains("README.md"), "{output}");
        }
    }

    #[test]
    fn quitting_a_pager_early_preserves_the_difference_exit_code() {
        let world = pending();
        // Exceed a pipe buffer, so the pager closing it can interrupt Git's writer.
        world.template.repo.write(
            "template/README.md.jinja",
            &"a long new line\n".repeat(20000),
        );
        world.template.repo.commit_all("docs: grow the diff");
        tpl(&world.project, &["update", "--defaults"]).success();
        for (args, expected) in [(vec!["diff"], 0), (vec!["diff", "--exit-code"], 1)] {
            let (code, output) = diff(&world, &args, Some("printf 'QUIT_PAGER\\n'; exit 0"));
            assert_eq!(code, expected, "{output}");
            assert!(output.contains("QUIT_PAGER"), "{output}");
            assert!(!output.contains("diff_display"), "{output}");
        }
    }
}

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
        .find(|l| l.split_whitespace().next() == Some(path))
        .unwrap_or_else(|| panic!("no line about {path} in:\n{output}"));
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn stat_counts_the_lines_a_merge_would_insert() {
    let world = pending();

    let output = tpl(&world.project, &["diff", "--stat"]).success();

    // The template appended a blank line and a sentence to `README.md`, and
    // added a one-line workflow.
    assert_eq!(stat_line(&output.all(), "README.md"), "README.md | 2 ++");
    assert_eq!(
        stat_line(&output.all(), ".github/workflows/release.yml"),
        ".github/workflows/release.yml | 1 +"
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

    assert_eq!(stat_line(&output.all(), "ci.yml"), "ci.yml | 5 -----");
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

    assert_eq!(stat_line(&forward.all(), "README.md"), "README.md | 2 ++");
    assert_eq!(stat_line(&reversed.all(), "README.md"), "README.md | 2 --");
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

    assert!(stat_line(&output.stdout, "logo.png").contains("Bin 0 -> 18 bytes"));
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

// ---------------------------------------------------------------------------
// `--dirty`: previewing an uncommitted template edit.
//
// Everything above diffs the rendered ref. These render the template's working
// tree on the fly, which is the loop a template author is actually in — and
// the reason `diff` had to learn to render at all.
// ---------------------------------------------------------------------------

/// The point of the flag: an edit that is not committed anywhere still shows up.
#[test]
fn dirty_previews_an_uncommitted_template_edit() {
    let world = World::new();
    world.init(&[]).success();

    world.template.repo.write(
        "template/README.md.jinja",
        "# {{ project_name }}\n\nnew line\n",
    );

    tpl(&world.project, &["diff", "--stat"])
        .success()
        .says("No differences.");

    tpl(&world.project, &["diff", "--dirty", "--stat"])
        .success()
        .says("README.md");
}

/// A preview is a rendering, and a rendering that moved the ref would be an
/// `update` nobody asked for — and one a later real update would have to
/// reconcile with.
#[test]
fn a_dirty_preview_does_not_move_the_rendered_ref() {
    let world = World::new();
    world.init(&[]).success();

    let before = world.project.rev_parse(&world.ref_name());
    let head_before = world.project.rev_parse("HEAD");

    world
        .template
        .repo
        .write("template/README.md.jinja", "# changed\n");

    tpl(&world.project, &["diff", "--dirty"]).success();

    assert_eq!(
        world.project.rev_parse(&world.ref_name()),
        before,
        "the preview advanced the rendered ref"
    );
    assert_eq!(
        world.project.rev_parse("HEAD"),
        head_before,
        "the preview moved HEAD"
    );
    assert!(
        world.project.status().is_empty(),
        "the worktree was touched"
    );
}

/// A preview answers the recorded questions, not a fresh questionnaire.
/// Without this it would hang for a non-interactive caller and re-ask an
/// interactive one just to look at a diff.
#[test]
fn a_dirty_preview_reuses_the_recorded_answers() {
    let world = World::new();
    world.init(&["--answer", "project_name=recorded"]).success();

    world
        .template
        .repo
        .write("template/README.md.jinja", "# {{ project_name }} edited\n");

    tpl(&world.project, &["show", "--dirty", "README.md"])
        .success()
        .says("recorded");
}

/// `show --dirty` reads one file out of the same preview.
#[test]
fn show_dirty_reads_from_the_uncommitted_template() {
    let world = World::new();
    world.init(&[]).success();

    world
        .template
        .repo
        .write("template/README.md.jinja", "# uncommitted marker\n");

    tpl(&world.project, &["show", "README.md"])
        .success()
        .silent_about("uncommitted marker");

    tpl(&world.project, &["show", "--dirty", "README.md"])
        .success()
        .says("uncommitted marker");
}

/// `status --dirty` is the cheap half: it reports against the working tree
/// rather than the committed revision, so an uncommitted edit reads as pending.
#[test]
fn status_dirty_reports_an_uncommitted_template_as_pending() {
    let world = World::new();
    world.init(&[]).success();

    tpl(&world.project, &["status"]).code(0);

    world
        .template
        .repo
        .write("template/README.md.jinja", "# changed\n");

    tpl(&world.project, &["status", "--dirty"]).code(2);
}

/// Git's own convention, so CI can assert "the template output has not
/// drifted" without parsing anything.
#[test]
fn exit_code_reports_a_difference() {
    let world = World::new();
    world.init(&[]).success();

    tpl(&world.project, &["diff", "--exit-code"]).code(0);

    world.move_template();
    tpl(&world.project, &["update"]).success();

    tpl(&world.project, &["diff", "--exit-code"]).code(1);
    // Without the flag it stays zero, because a diff is not a failure.
    tpl(&world.project, &["diff"]).code(0);
}

/// `--json` is a reporting mode, not a separate command: `--exit-code` must
/// still mean what it means everywhere else.
#[test]
fn diff_json_honours_exit_code() {
    let world = World::new();
    world.init(&[]).success();

    let json = tpl(&world.project, &["--json", "diff", "--exit-code"])
        .code(0)
        .json();
    assert_eq!(json["changes"], serde_json::json!([]));

    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();

    tpl(&world.project, &["--json", "diff", "--exit-code"]).code(1);
}

/// `--strict-answers` was accepted on `diff --dirty` but silently ignored —
/// only `render` enforced it.
#[test]
fn dirty_strict_answers_refuses_a_key_that_names_no_question() {
    let world = World::new();
    world.init(&[]).success();

    world
        .template
        .repo
        .write("template/README.md.jinja", "# changed\n");

    let output = tpl(
        &world.project,
        &[
            "--json",
            "diff",
            "--dirty",
            "--answer",
            "projct_name=oops",
            "--strict-answers",
        ],
    )
    .failure();

    assert_eq!(output.error_code(), "tpl::answers::unknown_key");
}
