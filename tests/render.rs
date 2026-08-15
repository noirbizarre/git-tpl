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
