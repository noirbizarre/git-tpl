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
    std::process::Command::new("git")
        .args(["clone", "-q", &source, &path.to_string_lossy()])
        .status()
        .expect("clone the template");

    let repo = Repo::at(path);
    repo.git(&["config", "user.name", "Test"]);
    repo.git(&["config", "user.email", "test@example.invalid"]);
    repo.git(&["config", "commit.gpgsign", "false"]);
    // The same pinning `Repo::configure` does, and for the same reason: a
    // Windows runner has `core.autocrlf=true` globally, so this checkout would
    // materialise CRLF while the patch we emit carries the template's LF, and
    // `git am` would fail to match its context lines. `git clone` does not go
    // through the harness's constructor, so it has to be said again here.
    repo.git(&["config", "core.autocrlf", "false"]);
    repo.git(&["config", "core.eol", "lf"]);
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
