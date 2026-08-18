//! `git tpl backport` — the patch that carries a local fix upstream.
//!
//! Real Git throughout, and the acceptance test really runs `git am`: the
//! claim the command makes is "this applies to your template", and only `git
//! am` can adjudicate that. A test that parsed the patch ourselves would be
//! testing our parser.

mod common;

use common::{Repo, World, tpl};

/// A template whose `README.md.jinja` has one substituted line and several
/// verbatim ones — the shape every case below needs.
fn world() -> World {
    World::with_template(
        r#"
name = "demo"

[questions.project_name]
type = "string"
prompt = "Project name"
default = "acme"
"#,
        &[
            (
                "README.md.jinja",
                "# {{ project_name }}\n\nA generated project.\n\nRun the tests before pushing.\n",
            ),
            // Not a `.jinja`, so it is copied byte-for-byte: the easy path,
            // and the one the ADR says must always work.
            ("ci.yml", "name: CI\non: push\njobs: {}\n"),
        ],
    )
}

/// A clone of the world's template, standing in for the one a contributor
/// keeps beside their project. `git am` runs here, never in the original.
fn clone_of_template(world: &World) -> Repo {
    let path = world.dir.path().join("upstream");
    let source = world.template.source();
    // `-c`, not `git config` afterwards: these have to be in force for the
    // clone's *checkout*. A Windows runner has `core.autocrlf=true` globally,
    // so without them the worktree and index materialise as CRLF while the
    // patch carries the template's LF, and `git am` refuses with
    // "does not match index". Setting them after the fact is too late — the
    // files are already on disk.
    std::process::Command::new("git")
        .args([
            "clone",
            "-q",
            "-c",
            "core.autocrlf=false",
            "-c",
            "core.eol=lf",
            &source,
            &path.to_string_lossy(),
        ])
        .status()
        .expect("clone the template");

    let repo = Repo::at(path);
    repo.git(&["config", "user.name", "Test"]);
    repo.git(&["config", "user.email", "test@example.invalid"]);
    repo.git(&["config", "commit.gpgsign", "false"]);
    repo
}

#[test]
fn an_edit_to_a_verbatim_file_produces_an_applicable_patch() {
    let world = world();
    world.init(&[]).success();

    // A fix in a file the template copies byte-for-byte.
    world
        .project
        .write("ci.yml", "name: CI\non: [push, pull_request]\njobs: {}\n");

    let output = tpl(&world.project, &["backport"]).success();
    assert!(
        output.stdout.contains("--- a/template/ci.yml"),
        "{}",
        output.transcript()
    );

    let upstream = clone_of_template(&world);
    let patch = upstream.path.join("backport.mbox");
    std::fs::write(&patch, &output.stdout).expect("write the patch");

    // The acceptance criterion, adjudicated by `git am` itself.
    let (ok, message) = upstream.try_git(&["am", &patch.to_string_lossy()]);
    assert!(ok, "git am refused the patch: {message}");
    assert_eq!(
        upstream.read("template/ci.yml"),
        "name: CI\non: [push, pull_request]\njobs: {}\n"
    );
}

#[test]
fn an_edit_to_a_verbatim_line_of_a_templated_file_keeps_the_placeholder() {
    let world = world();
    world.init(&[]).success();

    // The heading is substituted and untouched; the prose is verbatim and
    // changed. The patch must carry the second and preserve the first.
    world.project.write(
        "README.md",
        "# acme\n\nA generated project.\n\nRun the tests and the linter before pushing.\n",
    );

    let output = tpl(&world.project, &["backport"]).success();

    let upstream = clone_of_template(&world);
    let patch = upstream.path.join("backport.mbox");
    std::fs::write(&patch, &output.stdout).expect("write the patch");
    let (ok, message) = upstream.try_git(&["am", &patch.to_string_lossy()]);
    assert!(ok, "git am refused the patch: {message}");

    let source = upstream.read("template/README.md.jinja");
    assert!(
        source.starts_with("# {{ project_name }}\n"),
        "the placeholder was replaced with one user's answer: {source}"
    );
    assert!(
        source.contains("Run the tests and the linter before pushing.\n"),
        "the fix did not arrive: {source}"
    );
}

#[test]
fn an_edit_to_a_substituted_line_is_refused_by_name() {
    let world = world();
    world.init(&[]).success();

    // Renaming the project in the rendered file is a change of *answer*, not
    // of template. Guessing `{{ project_name }}` here would rename the
    // template's heading for everyone.
    world.project.write(
        "README.md",
        "# widgets\n\nA generated project.\n\nRun the tests before pushing.\n",
    );

    let output = tpl(&world.project, &["--json", "backport"]).failure();
    assert_eq!(output.error_code(), "tpl::backport::substituted_region");
}

/// The proof failing, which is the whole point of ADR-020.
///
/// The user wrote text that is itself Jinja syntax — documenting the template,
/// or a GitHub Actions `${{ }}`. Transposing it into the source succeeds, and
/// the source then renders that text away, so the patch would silently change
/// what the template produces. Only the re-render catches it: nothing about
/// the change looks wrong until you try it.
#[test]
fn a_patch_that_does_not_render_back_is_refused_by_name() {
    let world = world();
    world.init(&[]).success();
    world.project.write(
        "README.md",
        "# acme\n\nA generated project.\n\nSee {{ version }} for details.\n",
    );

    let output = tpl(&world.project, &["--json", "backport"]).failure();
    assert_eq!(output.error_code(), "tpl::backport::round_trip");
}

/// A verbatim file gets the same guarantee, by comparison rather than render.
///
/// `verify` short-circuits for a file the template copies: passing it through
/// MiniJinja here would render a template the real render never did, turning a
/// `${{ }}` in a workflow into a template expression.
#[test]
fn a_verbatim_file_is_checked_without_being_rendered() {
    let world = World::with_template(
        r#"
name = "demo"

[questions.project_name]
type = "string"
prompt = "Project name"
default = "acme"
"#,
        // Not a `.jinja`, and full of `${{ }}` — the case the suffix rule
        // exists for.
        &[(
            "ci.yml",
            "name: CI\njobs:\n  test:\n    steps:\n      - run: echo ${{ github.sha }}\n",
        )],
    );
    world.init(&[]).success();
    // The edited line is the `${{ }}` one, so it lands in the patch as a
    // changed line rather than sitting outside the context window.
    world.project.write(
        "ci.yml",
        "name: CI\njobs:\n  test:\n    steps:\n      - run: echo ${{ github.sha }} ${{ github.ref }}\n",
    );

    let output = tpl(&world.project, &["backport"]).success();
    // The `${{ }}` survives into the patch untouched, because nothing rendered
    // it on the way out.
    assert!(
        output
            .stdout
            .contains("+      - run: echo ${{ github.sha }} ${{ github.ref }}"),
        "{}",
        output.transcript()
    );

    let upstream = clone_of_template(&world);
    let patch = upstream.path.join("backport.mbox");
    std::fs::write(&patch, &output.stdout).expect("write the patch");
    let (ok, message) = upstream.try_git(&["am", &patch.to_string_lossy()]);
    assert!(ok, "git am refused the patch: {message}");
}

/// Not every non-text file has a NUL in the first 8 kB.
///
/// `is_binary` sniffs for NUL, which a Latin-1 file has none of. The UTF-8
/// decode is the second gate, and it must reach the same diagnostic — a
/// backport that panicked here would be worse than one that refused.
#[test]
fn a_file_that_is_not_utf8_is_refused_as_binary() {
    let world = World::with_template(
        r#"
name = "demo"

[questions.project_name]
type = "string"
prompt = "Project name"
default = "acme"
"#,
        &[("notes.txt", "plain\n")],
    );
    world.init(&[]).success();
    // `0xFF` is not valid UTF-8 and is not NUL.
    std::fs::write(world.project.path.join("notes.txt"), [b'a', 0xFF, b'\n'])
        .expect("write a latin-1 file");

    let output = tpl(&world.project, &["--json", "backport"]).failure();
    assert_eq!(output.error_code(), "tpl::backport::binary");
}

/// An added file gets the same binary check as a modified one.
#[test]
fn an_added_binary_file_is_refused_by_name() {
    let world = world();
    world.init(&[]).success();
    std::fs::write(world.project.path.join("logo.bin"), [0u8, 1, 2, 0])
        .expect("write a binary file");

    let output = tpl(&world.project, &["--json", "backport", "logo.bin"]).failure();
    assert_eq!(output.error_code(), "tpl::backport::binary");
}

/// `--output` naming something unwritable is a named failure, not a panic.
#[test]
fn an_unwritable_output_path_is_refused_by_name() {
    let world = world();
    world.init(&[]).success();
    world.project.write("ci.yml", "name: CI\non: [push]\n");

    // A directory is never a writable file, on every platform this ships to.
    let target = world.dir.path().join("a-directory");
    std::fs::create_dir_all(&target).expect("create the directory");

    let output = tpl(
        &world.project,
        &["--json", "backport", "-o", &target.to_string_lossy()],
    )
    .failure();
    assert_eq!(output.error_code(), "tpl::backport::output_write");
}

/// `--trust` reaches the op, and a template with no remote data still works.
#[test]
fn trust_is_accepted_and_changes_nothing_for_a_local_template() {
    let world = world();
    world.init(&[]).success();
    world.project.write("ci.yml", "name: CI\non: [push]\n");

    let output = tpl(&world.project, &["--json", "backport", "--trust"]).success();
    assert_eq!(output.json()["result"], "patched");
}

/// A template that opts into `strict = true` renders the check the same way.
///
/// The round-trip render has to use the manifest's undefined behaviour, or a
/// strict template would verify under lenient rules and emit a patch the real
/// render would reject.
#[test]
fn a_strict_template_is_verified_under_its_own_rules() {
    let world = World::with_template(
        r#"
name = "demo"
strict = true

[questions.project_name]
type = "string"
prompt = "Project name"
default = "acme"
"#,
        &[("README.md.jinja", "# {{ project_name }}\n\nprose\n")],
    );
    world.init(&[]).success();
    world
        .project
        .write("README.md", "# acme\n\nprose and more prose\n");

    let output = tpl(&world.project, &["backport"]).success();
    assert!(
        output.stdout.contains("+prose and more prose"),
        "{}",
        output.transcript()
    );

    // And a change that introduces an undefined name is refused, because the
    // strict render fails rather than quietly producing an empty string.
    world
        .project
        .write("README.md", "# acme\n\nprose {{ nope }}\n");
    let refused = tpl(&world.project, &["--json", "backport"]).failure();
    assert_eq!(refused.error_code(), "tpl::backport::round_trip");
}

#[test]
fn a_binary_change_is_refused_by_name() {
    let world = World::with_template(
        r#"
name = "demo"

[questions.project_name]
type = "string"
prompt = "Project name"
default = "acme"
"#,
        &[("logo.bin", "\u{0}\u{1}binary\u{0}\n")],
    );
    world.init(&[]).success();
    std::fs::write(world.project.path.join("logo.bin"), [0u8, 2, 3, 0, 9])
        .expect("write a binary file");

    let output = tpl(&world.project, &["--json", "backport"]).failure();
    assert_eq!(output.error_code(), "tpl::backport::binary");
}

#[test]
fn a_deleted_template_file_is_ignored_by_default() {
    let world = world();
    world.init(&[]).success();
    world.project.remove("ci.yml");

    let output = tpl(&world.project, &["--json", "backport"]).success();
    let json = output.json();
    assert_eq!(json["result"], "nothingToBackport");
    // Reported, though: an omission the user cannot see is worse than a noisy
    // one they can.
    assert_eq!(json["skipped"][0]["path"], "ci.yml");
    assert!(
        output.stderr.contains("skipped ci.yml"),
        "{}",
        output.stderr
    );
}

#[test]
fn a_project_added_file_is_backported_only_when_named() {
    let world = world();
    world.init(&[]).success();
    world.project.write("EXTRA.md", "Something new.\n");

    // Not template-owned, so silence by default.
    let untouched = tpl(&world.project, &["--json", "backport"]).success();
    assert_eq!(untouched.json()["result"], "nothingToBackport");

    // Named explicitly, it becomes a new template file — and *not* a `.jinja`,
    // because nothing was substituted into a file the template has never seen.
    let named = tpl(&world.project, &["backport", "EXTRA.md"]).success();
    assert!(
        named.stdout.contains("new file mode"),
        "{}",
        named.transcript()
    );
    assert!(
        named.stdout.contains("b/template/EXTRA.md\n"),
        "an added file must not gain a .jinja suffix: {}",
        named.transcript()
    );

    let upstream = clone_of_template(&world);
    let patch = upstream.path.join("backport.mbox");
    std::fs::write(&patch, &named.stdout).expect("write the patch");
    let (ok, message) = upstream.try_git(&["am", &patch.to_string_lossy()]);
    assert!(ok, "git am refused the patch: {message}");
    assert_eq!(upstream.read("template/EXTRA.md"), "Something new.\n");
}

#[test]
fn an_unchanged_project_produces_no_patch() {
    let world = world();
    world.init(&[]).success();

    let output = tpl(&world.project, &["backport"]).success();
    assert!(output.stdout.is_empty(), "{}", output.transcript());
    output.says("Nothing to backport");
}

/// A mode-only difference carries no content, so it carries nothing.
///
/// Windows cannot represent the executable bit, so *every* executable file a
/// template ships looks modified there. Emitting it produced a file section
/// with no hunks — a malformed patch that `git am` rejects outright, taking
/// the rest of the patch down with it.
#[test]
fn a_mode_only_difference_is_not_a_change_to_backport() {
    let world = World::with_template(
        r#"
name = "demo"

[questions.project_name]
type = "string"
prompt = "Project name"
default = "acme"
"#,
        &[
            ("run.sh", "#!/bin/sh\necho run\n"),
            ("ci.yml", "name: CI\n"),
        ],
    );
    world.template.repo.make_executable("template/run.sh");
    world
        .template
        .repo
        .commit_all("chore: make run.sh executable");
    world.init(&[]).success();

    // Drop the executable bit without touching a byte of the content.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = world.project.path.join("run.sh");
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o644);
        std::fs::set_permissions(&path, permissions).unwrap();
    }

    let output = tpl(&world.project, &["--json", "backport"]).success();
    let json = output.json();
    assert_eq!(json["result"], "nothingToBackport");
    assert_eq!(json["patch"], "");

    // And with a real change beside it, the patch carries only that file —
    // never an empty section for the mode.
    world.project.write("ci.yml", "name: CI\non: [push]\n");
    let output = tpl(&world.project, &["backport"]).success();
    assert!(
        !output.stdout.contains("run.sh"),
        "a mode-only difference leaked into the patch: {}",
        output.transcript()
    );

    let upstream = clone_of_template(&world);
    let patch = upstream.path.join("backport.mbox");
    std::fs::write(&patch, &output.stdout).expect("write the patch");
    let (ok, message) = upstream.try_git(&["am", &patch.to_string_lossy()]);
    assert!(ok, "git am refused the patch: {message}");
}

#[test]
fn excluded_paths_are_left_out() {
    let world = world();
    world.init(&[]).success();
    world.project.write("ci.yml", "name: CI\non: [push]\n");

    let output = tpl(&world.project, &["backport", "--exclude", "ci.yml"]).success();
    assert!(output.stdout.is_empty(), "{}", output.transcript());
}

#[test]
fn backport_writes_nothing_anywhere() {
    let world = world();
    world.init(&[]).success();
    world.project.write("ci.yml", "name: CI\non: [push]\n");

    let project_before = world.project.working_state();
    let template_before = world.template.repo.working_state();

    tpl(&world.project, &["backport"]).success();

    // Invariant 1 for the project, and the issue's third acceptance criterion
    // for the template: a resolved template is a throwaway clone, and writing
    // into it writes into a directory about to be deleted.
    assert_eq!(world.project.working_state(), project_before);
    assert_eq!(world.template.repo.working_state(), template_before);
}

#[test]
fn answers_that_no_longer_reproduce_the_ref_are_refused() {
    let world = world();
    world.init(&[]).success();
    world.project.write("ci.yml", "name: CI\non: [push]\n");

    // Editing the recorded answer without re-rendering: the ref now holds a
    // tree the answers do not produce, so every line number would be measured
    // against the wrong file.
    let config = world.project.read(".config/git.tpl.toml");
    world.project.write(
        ".config/git.tpl.toml",
        &config.replace("acme", "something-else"),
    );

    let output = tpl(&world.project, &["--json", "backport"]).failure();
    assert_eq!(output.error_code(), "tpl::backport::stale_rendering");
}

#[test]
fn the_json_payload_carries_the_patch() {
    let world = world();
    world.init(&[]).success();
    world.project.write("ci.yml", "name: CI\non: [push]\n");

    let output = tpl(&world.project, &["--json", "backport"]).success();
    let json = output.json();
    let data = &json;

    assert_eq!(data["result"], "patched");
    // The patch travels in the payload, because `--json` means stdout is one
    // JSON object.
    assert!(
        data["patch"]
            .as_str()
            .expect("a patch string")
            .contains("--- a/template/ci.yml"),
        "{data}"
    );
    assert_eq!(data["files"][0]["rendered"], "ci.yml");
    assert_eq!(data["files"][0]["source"], "template/ci.yml");
    assert_eq!(data["files"][0]["added"], false);
    assert!(
        data["applyCommand"]
            .as_str()
            .expect("an apply command")
            .contains(" am"),
        "{data}"
    );
}

#[test]
fn the_apply_command_names_a_local_template_clone() {
    let world = world();
    world.init(&[]).success();
    world.project.write("ci.yml", "name: CI\non: [push]\n");

    let output = tpl(&world.project, &["--json", "backport"]).success();
    let command = output.json()["applyCommand"]
        .as_str()
        .expect("an apply command")
        .to_string();

    // A local source is a directory we can name. It must not be the resolved
    // template's workdir, which is a throwaway clone under the temp dir.
    assert!(command.contains("git tpl backport | git -C "), "{command}");
    assert!(command.ends_with(" am"), "{command}");
    assert!(
        !command.contains("<your-template-clone>"),
        "a local template should be named: {command}"
    );
}

#[test]
fn an_output_file_holds_the_patch_and_stdout_stays_empty() {
    let world = world();
    world.init(&[]).success();
    world.project.write("ci.yml", "name: CI\non: [push]\n");

    let target = world.dir.path().join("fix.mbox");
    let output = tpl(
        &world.project,
        &["backport", "-o", &target.to_string_lossy()],
    )
    .success();
    assert!(output.stdout.is_empty(), "{}", output.transcript());

    let patch = std::fs::read_to_string(&target).expect("the patch file");
    assert!(patch.contains("--- a/template/ci.yml"), "{patch}");
    assert!(patch.starts_with("From "), "{patch}");
}

#[test]
fn a_backported_fix_comes_back_through_update() {
    let world = world();
    world.init(&[]).success();
    world
        .project
        .write("ci.yml", "name: CI\non: [push, pull_request]\njobs: {}\n");
    world.project.commit_all("fix: run CI on pull requests");

    // Out.
    let output = tpl(&world.project, &["backport"]).success();

    // Applied upstream, by `git am`, in the template itself — which is what a
    // maintainer merging the contribution would end up with.
    let patch = world.dir.path().join("fix.mbox");
    std::fs::write(&patch, &output.stdout).expect("write the patch");
    let (ok, message) = world
        .template
        .repo
        .try_git(&["am", &patch.to_string_lossy()]);
    assert!(ok, "git am refused the patch: {message}");

    // And back. The fix now arrives from upstream, and merges without
    // conflicting with the identical change already in the project.
    tpl(&world.project, &["update"]).success();
    tpl(&world.project, &["merge"]).success();
    assert_eq!(
        world.project.read("ci.yml"),
        "name: CI\non: [push, pull_request]\njobs: {}\n"
    );

    // The loop is closed: there is nothing left to send.
    let again = tpl(&world.project, &["--json", "backport"]).success();
    assert_eq!(again.json()["result"], "nothingToBackport");
}

// ---- un-substitution, ADR-022 ---------------------------------------------

/// A template whose lines mix substituted values with editable prose, which is
/// the shape #66 exists for. Deliberately separate from [`world`]: the cases
/// above pin what happens *without* un-substitution and must keep doing so.
fn substituting_world() -> World {
    World::with_template(
        r#"
name = "demo"

[questions.project_name]
type = "string"
prompt = "Project name"
default = "acme"

[questions.author]
type = "string"
prompt = "Author"
default = "June"
"#,
        &[(
            "README.md.jinja",
            "# {{ project_name }} — a service\n\nWritten by {{ author }} in June.\n\nRun the tests.\n",
        )],
    )
}

/// The acceptance case: the change is beside the placeholder, not in it.
#[test]
fn an_edit_beside_a_substitution_keeps_the_placeholder_and_carries_the_change() {
    let world = substituting_world();
    world.init(&[]).success();

    world.project.write(
        "README.md",
        "# acme — a web service\n\nWritten by June in June.\n\nRun the tests.\n",
    );

    let output = tpl(&world.project, &["backport", "--unsubstitute"]).success();

    let upstream = clone_of_template(&world);
    let patch = upstream.path.join("backport.mbox");
    std::fs::write(&patch, &output.stdout).expect("write the patch");
    let (ok, message) = upstream.try_git(&["am", &patch.to_string_lossy()]);
    assert!(ok, "git am refused the patch: {message}");

    let source = upstream.read("template/README.md.jinja");
    assert!(
        source.starts_with("# {{ project_name }} — a web service\n"),
        "the placeholder did not survive the reversal: {source}"
    );
}

/// The `June` case ADR-020 says a substitution table cannot get right.
///
/// `author` is the month the template hard-codes. Nothing searches for the
/// value, so the coincidence is literal text and edits to it carry cleanly.
#[test]
fn a_value_that_coincides_with_literal_text_is_not_reversed_by_accident() {
    let world = substituting_world();
    world.init(&[]).success();

    world.project.write(
        "README.md",
        "# acme — a service\n\nWritten by June in July.\n\nRun the tests.\n",
    );

    let output = tpl(&world.project, &["--json", "backport", "--unsubstitute"]).success();
    let payload = output.json();
    let patch = payload["patch"].as_str().expect("a patch");
    assert!(
        patch.contains("+Written by {{ author }} in July."),
        "the coincidence was un-substituted, or the change was lost: {patch}"
    );
}

/// The other half of the same line: editing the *author* is an answer change.
#[test]
fn editing_the_value_itself_is_still_refused_by_name() {
    let world = substituting_world();
    world.init(&[]).success();

    world.project.write(
        "README.md",
        "# acme — a service\n\nWritten by Ada in June.\n\nRun the tests.\n",
    );

    let output = tpl(&world.project, &["--json", "backport", "--unsubstitute"]).failure();
    assert_eq!(output.error_code(), "tpl::backport::substituted_region");
}

/// Without the flag, and with nobody to ask, nothing is reversed.
///
/// This is the guarantee that a script's behaviour did not change under it:
/// the same edit that succeeds above refuses here, with the ADR-020 code.
#[test]
fn un_substitution_is_not_attempted_when_there_is_nobody_to_ask() {
    let world = substituting_world();
    world.init(&[]).success();

    world.project.write(
        "README.md",
        "# acme — a web service\n\nWritten by June in June.\n\nRun the tests.\n",
    );

    let output = tpl(&world.project, &["--json", "backport"]).failure();
    assert_eq!(output.error_code(), "tpl::backport::substituted_region");
}

/// A reversal is reported, not merely counted.
///
/// A patch that reversed a substitution changes what the template produces for
/// every project. A consumer that cannot see which lines they were cannot
/// review them.
#[test]
fn the_json_payload_names_every_reversed_line() {
    let world = substituting_world();
    world.init(&[]).success();

    world.project.write(
        "README.md",
        "# acme — a web service\n\nWritten by June in June.\n\nRun the tests.\n",
    );

    let output = tpl(&world.project, &["--json", "backport", "--unsubstitute"]).success();
    let payload = output.json();
    let reversed = payload["unsubstituted"].as_array().expect("the array");

    assert_eq!(reversed.len(), 1, "{payload}");
    assert_eq!(reversed[0]["path"], "README.md");
    assert_eq!(reversed[0]["source"], "README.md.jinja");
    assert_eq!(reversed[0]["line"], 1);
    assert_eq!(
        reversed[0]["patched"],
        "# {{ project_name }} — a web service"
    );
    assert_eq!(reversed[0]["expressions"][0], "{{ project_name }}");
}

/// A patch that round-trips and is still wrong.
///
/// Appending `.0` to a rendered version sits against the value, and placing it
/// in the literal gives `{{ version }}.0` — correct for this user, and wrong
/// for every other project. The round trip cannot tell; the slider can.
#[test]
fn an_edit_that_could_have_slid_into_a_value_is_refused_by_name() {
    let world = World::with_template(
        r#"
name = "demo"

[questions.version]
type = "string"
prompt = "Version"
default = "1.0"
"#,
        &[(
            "app.toml.jinja",
            "version = \"{{ version }}\"\nname = \"x\"\n",
        )],
    );
    world.init(&[]).success();

    world
        .project
        .write("app.toml", "version = \"1.0.0\"\nname = \"x\"\n");

    let output = tpl(&world.project, &["--json", "backport", "--unsubstitute"]).failure();
    assert_eq!(output.error_code(), "tpl::backport::substituted_region");
}

/// A line inside a loop has no line-local provenance to reverse.
///
/// The source line renders against a binding that is not in the context at
/// all, so the reassembly cannot reproduce the rendered line and the reversal
/// is refused rather than applied to every iteration.
#[test]
fn a_line_inside_a_loop_is_refused_by_name() {
    let world = World::with_template(
        r#"
name = "demo"

[questions.project_name]
type = "string"
prompt = "Project name"
default = "acme"

[computed]
items = "{{ ['one', 'two'] }}"
"#,
        &[(
            "list.md.jinja",
            "# {{ project_name }}\n{% for item in items %}\n- {{ item }} ok\n{% endfor %}\n",
        )],
    );
    world.init(&[]).success();

    let rendered = world.project.read("list.md");
    world
        .project
        .write("list.md", &rendered.replace("one ok", "one fine"));

    let output = tpl(&world.project, &["--json", "backport", "--unsubstitute"]).failure();
    assert_eq!(output.error_code(), "tpl::backport::substituted_region");
}

/// The full loop, with a reversal in it.
///
/// The only test that would catch a patch which round-trips locally and then
/// breaks on the next `update`.
#[test]
fn an_un_substituted_fix_comes_back_through_update() {
    let world = substituting_world();
    world.init(&[]).success();
    world.project.write(
        "README.md",
        "# acme — a web service\n\nWritten by June in June.\n\nRun the tests.\n",
    );
    world.project.commit_all("docs: call it a web service");

    let output = tpl(&world.project, &["backport", "--unsubstitute"]).success();

    let patch = world.dir.path().join("fix.mbox");
    std::fs::write(&patch, &output.stdout).expect("write the patch");
    let (ok, message) = world
        .template
        .repo
        .try_git(&["am", &patch.to_string_lossy()]);
    assert!(ok, "git am refused the patch: {message}");

    tpl(&world.project, &["update"]).success();
    tpl(&world.project, &["merge"]).success();
    assert_eq!(
        world.project.read("README.md"),
        "# acme — a web service\n\nWritten by June in June.\n\nRun the tests.\n"
    );

    // The loop is closed: the template now produces what the project has.
    let again = tpl(&world.project, &["--json", "backport", "--unsubstitute"]).success();
    assert_eq!(again.json()["result"], "nothingToBackport");
}

/// A file that reverses a substitution *and* then fails to render back.
///
/// The reversal on line 1 is fine; the edit on line 5 is text that is itself
/// Jinja, so the patched source renders it away and the round trip fails.
/// Reporting `round_trip` here would tell the user the patch was built and
/// re-rendered — true, but it sends them looking at the wrong line. The
/// command re-asks without un-substitution and reports that answer instead.
#[test]
fn a_reversal_that_does_not_render_back_reports_the_honest_refusal() {
    let world = substituting_world();
    world.init(&[]).success();

    world.project.write(
        "README.md",
        "# acme — a web service\n\nWritten by June in June.\n\nRun {{ nope }}.\n",
    );

    let output = tpl(&world.project, &["--json", "backport", "--unsubstitute"]).failure();
    assert_eq!(output.error_code(), "tpl::backport::substituted_region");
}
