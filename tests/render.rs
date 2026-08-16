//! `git tpl render` — a template, an answer set, a directory.
//!
//! The point of every test here is what is *absent*: no repository, no ref, no
//! `.config/git.tpl.toml`. A test that ran inside a repository would pass
//! whether or not the command needed one.

mod common;

use common::{Template, World, tpl_outside};

/// A scratch directory that is deliberately not a repository.
struct Scratch {
    dir: tempfile::TempDir,
    config: tempfile::TempDir,
}

impl Scratch {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("tempdir"),
            config: tempfile::tempdir().expect("config dir"),
        }
    }

    fn run(&self, args: &[&str]) -> common::Output {
        tpl_outside(self.dir.path(), self.config.path(), args)
    }

    fn out(&self) -> std::path::PathBuf {
        self.dir.path().join("out")
    }

    /// Point `core.excludesFile` at a global ignore file holding `rules`.
    fn global_gitignore(&self, rules: &str) -> &Self {
        common::global_gitignore(self.config.path(), rules);
        self
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.out().join(path))
            .unwrap_or_else(|e| panic!("read {path}: {e}"))
    }
}

fn template() -> (tempfile::TempDir, Template) {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = Template::standard(dir.path());
    (dir, template)
}

#[test]
fn a_template_renders_into_a_directory_without_a_repository() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    scratch
        .run(&[
            "render",
            &template.source(),
            "--output",
            scratch.out().to_str().unwrap(),
            "--defaults",
        ])
        .success();

    assert!(scratch.out().join("Cargo.toml").exists());
    assert!(scratch.out().join("src/lib.rs").exists());
    assert!(scratch.read("Cargo.toml").contains("demo"));
}

/// The whole claim of the command. `init` needs a repository at both ends;
/// this needs one only at the template end.
#[test]
fn rendering_creates_no_repository() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    scratch
        .run(&[
            "render",
            &template.source(),
            "--output",
            scratch.out().to_str().unwrap(),
            "--defaults",
        ])
        .success();

    assert!(!scratch.out().join(".git").exists());
    assert!(!scratch.dir.path().join(".git").exists());
    assert!(!scratch.out().join(".config/git.tpl.toml").exists());
}

#[test]
fn answers_come_from_a_file() {
    let (_keep, template) = template();
    let scratch = Scratch::new();
    let answers = scratch.dir.path().join("answers.toml");
    std::fs::write(&answers, "project_name = \"chosen\"\n").expect("write answers");

    scratch
        .run(&[
            "render",
            &template.source(),
            "--output",
            scratch.out().to_str().unwrap(),
            "--answers-from",
            answers.to_str().unwrap(),
            "--defaults",
        ])
        .success();

    assert!(scratch.read("Cargo.toml").contains("chosen"));
}

/// A file not named `.jinja` is copied byte-for-byte, which is what lets a
/// template ship a GitHub Actions workflow full of `${{ }}`. `templated`
/// reports which, because the two are indistinguishable in the output.
#[test]
fn the_json_payload_says_which_files_were_templated() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    let output = scratch
        .run(&[
            "--json",
            "render",
            &template.source(),
            "--output",
            scratch.out().to_str().unwrap(),
            "--defaults",
        ])
        .success();

    let json = output.json();
    assert_eq!(json["ok"], true);
    assert_eq!(json["template"]["name"], "rust-library");

    let files = json["files"].as_array().expect("files");
    let templated = |path: &str| {
        files
            .iter()
            .find(|f| f["path"] == path)
            .unwrap_or_else(|| panic!("{path} not rendered"))["templated"]
            .as_bool()
            .expect("bool")
    };

    assert!(templated("Cargo.toml"), "Cargo.toml.jinja was rendered");
    assert!(!templated("ci.yml"), "ci.yml is copied verbatim");
    assert!(
        scratch.read("ci.yml").contains("${{ github.sha }}"),
        "a verbatim copy keeps its GitHub expressions"
    );
}

/// Rendering over a previous run would leave a file the template no longer
/// produces, and the author would conclude their conditional works.
#[test]
fn a_non_empty_output_directory_is_refused() {
    let (_keep, template) = template();
    let scratch = Scratch::new();
    std::fs::create_dir_all(scratch.out()).expect("mkdir");
    std::fs::write(scratch.out().join("existing"), "keep me").expect("write");

    scratch
        .run(&[
            "render",
            &template.source(),
            "--output",
            scratch.out().to_str().unwrap(),
            "--defaults",
        ])
        .failure()
        .says("--force");
}

#[test]
fn force_replaces_the_output_rather_than_merging_into_it() {
    let (_keep, template) = template();
    let scratch = Scratch::new();
    std::fs::create_dir_all(scratch.out()).expect("mkdir");
    std::fs::write(scratch.out().join("stale"), "gone").expect("write");

    scratch
        .run(&[
            "render",
            &template.source(),
            "--output",
            scratch.out().to_str().unwrap(),
            "--force",
            "--defaults",
        ])
        .success();

    assert!(scratch.out().join("Cargo.toml").exists());
    assert!(
        !scratch.out().join("stale").exists(),
        "a stale file survived, so a removed template file would look present"
    );
}

/// Not a directory at all. The error must name the path and say what failed to
/// happen to it — "read `/tmp/x/out`" — because the alternative is a bare
/// `NotADirectory` from somewhere inside a command that touched four paths.
#[test]
fn an_output_path_that_is_a_file_is_refused_with_the_path_that_failed() {
    let (_keep, template) = template();
    let scratch = Scratch::new();
    std::fs::write(scratch.out(), "I am a file, not a directory").expect("write");

    let output = scratch
        .run(&[
            "render",
            &template.source(),
            "--output",
            scratch.out().to_str().unwrap(),
            "--defaults",
        ])
        .failure();

    output.says("out");
    assert!(
        scratch.out().is_file(),
        "the output path was clobbered rather than refused"
    );
}

#[test]
fn the_executable_bit_survives() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    scratch
        .run(&[
            "render",
            &template.source(),
            "--output",
            scratch.out().to_str().unwrap(),
            "--defaults",
        ])
        .success();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(scratch.out().join("run.sh"))
            .expect("run.sh")
            .permissions()
            .mode();
        assert!(mode & 0o111 != 0, "run.sh lost its executable bit");
    }
}

/// `--dirty` is the edit-and-see loop: without it an author has to commit
/// before they can find out whether the edit was right.
#[test]
fn dirty_renders_the_uncommitted_template() {
    let (_keep, template) = template();
    let scratch = Scratch::new();
    template.repo.write(
        "template/README.md.jinja",
        "# uncommitted {{ project_name }}\n",
    );

    scratch
        .run(&[
            "render",
            &template.source(),
            "--dirty",
            "--output",
            scratch.out().to_str().unwrap(),
            "--defaults",
        ])
        .success();

    assert!(scratch.read("README.md").contains("uncommitted"));
}

/// A `local` data source is resolved against the project root, and there is no
/// project. Guessing at the working directory would make the same template
/// render differently depending on where the command was run.
#[test]
fn a_local_data_source_is_refused_rather_than_guessed_at() {
    let world = World::with_template(
        r#"
name = "needs-project"

[data.things]
source = "./things.toml"

[questions.thing]
type = "choice"
choices_from = "data.things.ids"
"#,
        &[("file.txt.jinja", "{{ thing }}\n")],
    );
    let scratch = Scratch::new();

    let output = scratch
        .run(&[
            "--json",
            "render",
            &world.template.source(),
            "--output",
            scratch.out().to_str().unwrap(),
            "--defaults",
        ])
        .failure();

    assert_eq!(output.error_code(), "tpl::data::needs_project");
}

/// The failure envelope is the point of `--json`: a caller has to be able to
/// branch on *which* thing went wrong.
#[test]
fn a_failure_reports_its_diagnostic_code_as_json() {
    let scratch = Scratch::new();

    let output = scratch
        .run(&[
            "--json",
            "render",
            "/nonexistent/template",
            "--output",
            scratch.out().to_str().unwrap(),
            "--defaults",
        ])
        .failure();

    assert_eq!(output.json()["ok"], false);
    assert!(
        output.error_code().starts_with("tpl::"),
        "expected a tpl:: code, got {}",
        output.error_code()
    );
}

/// Lenient is still the default, so an upgrade does not break a template that
/// renders today. The lint reports the same name as a warning meanwhile.
#[test]
fn an_undeclared_name_renders_empty_by_default() {
    let world = World::with_template(
        r#"
name = "lenient"

[questions.project_name]
type = "string"
default = "demo"
"#,
        &[("Cargo.toml.jinja", "name = \"{{ projct_name }}\"\n")],
    );
    let scratch = Scratch::new();

    scratch
        .run(&[
            "render",
            &world.template.source(),
            "--output",
            scratch.out().to_str().unwrap(),
            "--defaults",
        ])
        .success();

    // The failure this is all about: valid TOML, an empty name, exit zero.
    assert_eq!(scratch.read("Cargo.toml"), "name = \"\"\n");
}

#[test]
fn strict_makes_an_undeclared_name_fail_the_render() {
    let world = World::with_template(
        r#"
name = "strict"
strict = true

[questions.project_name]
type = "string"
default = "demo"
"#,
        &[("Cargo.toml.jinja", "name = \"{{ projct_name }}\"\n")],
    );
    let scratch = Scratch::new();

    let output = scratch
        .run(&[
            "--json",
            "render",
            &world.template.source(),
            "--output",
            scratch.out().to_str().unwrap(),
            "--defaults",
        ])
        .failure();

    assert_eq!(output.error_code(), "tpl::render::content");
    // The cause chain is the point: the outer error names the file, and only
    // the one beneath it names the expression.
    let json = output.json();
    assert_eq!(json["error"]["causes"][0]["code"], "tpl::eval::expression");
}

/// `| default('')` is how a template says a name is optional on purpose, and
/// it has to keep working under `strict`.
#[test]
fn strict_allows_an_explicit_default() {
    let world = World::with_template(
        r#"
name = "strict-optional"
strict = true

[questions.project_name]
type = "string"
default = "demo"
"#,
        &[(
            "Cargo.toml.jinja",
            "name = \"{{ project_name }}\"\nextra = \"{{ maybe | default('') }}\"\n",
        )],
    );
    let scratch = Scratch::new();

    scratch
        .run(&[
            "render",
            &world.template.source(),
            "--output",
            scratch.out().to_str().unwrap(),
            "--defaults",
        ])
        .success();

    assert!(scratch.read("Cargo.toml").contains("name = \"demo\""));
}

/// A typo'd key silently swaps in the default. For a boolean that deletes a
/// whole conditional subtree while the warning scrolls past.
#[test]
fn strict_answers_refuses_a_key_that_names_no_question() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    // Lenient by default: reported, but not fatal.
    scratch
        .run(&[
            "render",
            &template.source(),
            "--output",
            scratch.out().to_str().unwrap(),
            "--answer",
            "projct_name=oops",
            "--defaults",
        ])
        .success()
        .says("answers ignored");

    let output = scratch
        .run(&[
            "--json",
            "render",
            &template.source(),
            "--output",
            scratch.out().to_str().unwrap(),
            "--force",
            "--answer",
            "projct_name=oops",
            "--defaults",
            "--strict-answers",
        ])
        .failure();

    assert_eq!(output.error_code(), "tpl::answers::unknown_key");
    let json = output.json();
    let help = json["error"]["help"].as_str().expect("help");
    assert!(help.contains("project_name"), "no suggestion in: {help}");
}

/// The other half of the flag, and the one that would go unnoticed if it broke:
/// a strict run whose answers are all real must be an ordinary run. A check
/// that refuses correct input is worse than no check.
#[test]
fn strict_answers_accepts_an_answer_set_that_names_only_real_questions() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    scratch
        .run(&[
            "render",
            &template.source(),
            "--output",
            scratch.out().to_str().unwrap(),
            "--answer",
            "project_name=strictly",
            "--defaults",
            "--strict-answers",
        ])
        .success()
        .silent_about("answers ignored");

    assert!(scratch.read("Cargo.toml").contains("strictly"));
}

/// The ignore stack includes `core.excludesFile`, so a global rule set years
/// ago on an unrelated project can remove a file the author can see on disk.
/// Inside a render there is no `git status` to explain the absence.
#[test]
fn dirty_reports_what_gitignore_removed_from_the_render() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    template.repo.write(".gitignore", "*.local\n");
    template.repo.write("template/secret.local", "hidden\n");
    template.repo.commit_all("chore: ignore local files");

    let output = scratch
        .run(&[
            "--json",
            "render",
            &template.source(),
            "--dirty",
            "--output",
            scratch.out().to_str().unwrap(),
            "--defaults",
        ])
        .success();

    assert!(!scratch.out().join("secret.local").exists());
    let json = output.json();
    let skipped = json["skippedByGitignore"].as_array().expect("skipped");
    assert!(
        skipped.iter().any(|p| p == "template/secret.local"),
        "the absence went unexplained: {skipped:?}"
    );
    // And loudly enough to be seen, since it is stderr a human reads.
    output.says("skipped by .gitignore");
}

/// The count alone tells the author something vanished; only the paths tell
/// them *what*, and a template with a dozen ignored files is exactly when the
/// count stops being enough.
#[test]
fn verbose_lists_the_files_gitignore_removed() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    template.repo.write(".gitignore", "*.local\n");
    template.repo.write("template/secret.local", "hidden\n");
    template.repo.commit_all("chore: ignore local files");

    let out = scratch.out();
    let out = out.to_str().unwrap();

    // Without `-v`, the path is withheld and the way to get it is offered.
    scratch
        .run(&[
            "render",
            &template.source(),
            "--dirty",
            "--output",
            out,
            "--defaults",
            "--force",
        ])
        .success()
        .silent_about("template/secret.local")
        .says("run with -v to list them");

    scratch
        .run(&[
            "render",
            &template.source(),
            "--dirty",
            "--output",
            out,
            "--defaults",
            "--force",
            "-v",
        ])
        .success()
        .says("template/secret.local");
}

/// The bug behind #51. A global `core.excludesFile` hides `mise.toml` on the
/// assumption that mise configuration is personal; a project that commits one
/// re-includes it with `!mise.toml`, and any template rendering a `mise.toml`
/// ships that negation. Git honours it and stages the file; libgit2 does not,
/// and dropped it from the render.
///
/// A rendering that differs by flag is the one thing `--dirty` is careful
/// about, so this is pinned rather than left to the ignore crate.
#[test]
fn a_gitignore_negation_overrides_a_global_ignore_rule() {
    let (_keep, template) = template();
    let scratch = Scratch::new();
    scratch.global_gitignore("mise.toml\nmise.lock\n");

    // The negation lives beside the files it re-includes, as it would in a
    // rendered project.
    template.repo.write("template/.gitignore", "!mise.toml\n");
    template.repo.write("template/mise.toml", "[tools]\n");
    template.repo.write("template/mise.lock", "lock\n");
    template.repo.commit_all("feat: render a mise.toml");

    let output = scratch
        .run(&[
            "--json",
            "render",
            &template.source(),
            "--dirty",
            "--output",
            scratch.out().to_str().unwrap(),
            "--defaults",
        ])
        .success();

    assert!(
        scratch.out().join("mise.toml").exists(),
        "the negation lost to the global rule, as it did before #51"
    );
    let json = output.json();
    let skipped = json["skippedByGitignore"].as_array().expect("skipped");
    assert!(
        !skipped.iter().any(|p| p == "template/mise.toml"),
        "reported as ignored despite the negation: {skipped:?}"
    );
    // The other half of the fix: the global rule still governs everything the
    // template did not re-include, or this would have been a blunt instrument.
    assert!(!scratch.out().join("mise.lock").exists());
    assert!(skipped.iter().any(|p| p == "template/mise.lock"));
}

/// Git's rule is that a file cannot be re-included once one of its parent
/// directories is excluded, so an ignored directory is pruned rather than
/// descended into. A walk that recursed looking for negations inside would
/// resurrect files `git add -A` leaves alone.
#[test]
fn an_ignored_directory_is_pruned_rather_than_descended_into() {
    let (_keep, template) = template();
    let scratch = Scratch::new();

    template.repo.write(".gitignore", "build/\n!build/keep\n");
    template.repo.write("template/build/keep", "not kept\n");
    // A *file* named `build`. `build/` names a directory only, which the walk
    // can know only by stat-ing before it asks the ignore stack.
    template
        .repo
        .write("template/sub/build", "a file, not a directory\n");
    template.repo.commit_all("chore: ignore build output");

    scratch
        .run(&[
            "render",
            &template.source(),
            "--dirty",
            "--output",
            scratch.out().to_str().unwrap(),
            "--defaults",
        ])
        .success();

    assert!(!scratch.out().join("build/keep").exists());
    assert_eq!(scratch.read("sub/build"), "a file, not a directory\n");
}
