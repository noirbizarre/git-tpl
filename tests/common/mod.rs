//! Shared harness for the integration tests.
//!
//! Builds **real** Git repositories in temporary directories and drives the
//! real binary. Nothing about Git is mocked: the entire premise of the project
//! is that Git's behaviour is the behaviour, so a test against a stub would be
//! testing the stub.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::*;

/// A Git repository under test.
pub struct Repo {
    pub path: PathBuf,
    /// Kept so a repository that owns its temporary directory outlives it.
    /// `None` when the directory belongs to a [`World`].
    pub _dir: Option<tempfile::TempDir>,
}

impl Repo {
    /// A new repository with an identity configured.
    pub fn init(name: &str) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(name);
        std::fs::create_dir_all(&path).expect("create repo dir");

        let repo = Self {
            path,
            _dir: Some(dir),
        };
        repo.git(&["init", "-q", "-b", "main"]);
        // libgit2 refuses to build a signature without an identity, and a
        // fresh CI runner has none. Set per-repository rather than globally so
        // the tests cannot depend on — or disturb — the developer's own config.
        repo.git(&["config", "user.name", "Test"]);
        repo.git(&["config", "user.email", "test@example.invalid"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo
    }

    /// A repository inside an existing directory, sharing its lifetime.
    pub fn init_in(parent: &Path, name: &str) -> Self {
        let path = parent.join(name);
        std::fs::create_dir_all(&path).expect("create repo dir");
        let repo = Self { path, _dir: None };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.name", "Test"]);
        repo.git(&["config", "user.email", "test@example.invalid"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo
    }

    /// Write a file, creating parent directories.
    pub fn write(&self, relative: &str, content: &str) -> &Self {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(&path, content).expect("write file");
        self
    }

    /// Make a file executable. No-op off Unix, where Git records no such bit.
    #[cfg(unix)]
    pub fn make_executable(&self, relative: &str) -> &Self {
        use std::os::unix::fs::PermissionsExt;
        let path = self.path.join(relative);
        let mut permissions = std::fs::metadata(&path).expect("stat").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).expect("chmod");
        self
    }

    #[cfg(not(unix))]
    pub fn make_executable(&self, _relative: &str) -> &Self {
        self
    }

    /// Read a file.
    pub fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.path.join(relative))
            .unwrap_or_else(|e| panic!("read {relative}: {e}"))
    }

    /// Whether a path exists.
    pub fn exists(&self, relative: &str) -> bool {
        self.path.join(relative).exists()
    }

    /// Delete a file.
    pub fn remove(&self, relative: &str) -> &Self {
        std::fs::remove_file(self.path.join(relative)).expect("remove file");
        self
    }

    /// Stage everything and commit.
    pub fn commit_all(&self, message: &str) -> &Self {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
        self
    }

    /// Run a Git command, asserting it succeeded.
    pub fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.path)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?} failed in {}:\n{}",
            self.path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Run a Git command, returning success and output whatever happens.
    pub fn try_git(&self, args: &[&str]) -> (bool, String) {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.path)
            .output()
            .expect("run git");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        (output.status.success(), combined)
    }

    /// Resolve a revision to a full SHA.
    pub fn rev_parse(&self, revision: &str) -> String {
        self.git(&["rev-parse", revision])
    }

    /// Whether a ref exists.
    pub fn has_ref(&self, name: &str) -> bool {
        self.try_git(&["rev-parse", "--verify", "--quiet", name]).0
    }

    /// The full message of a commit.
    pub fn commit_message(&self, revision: &str) -> String {
        self.git(&["log", "-1", "--format=%B", revision])
    }

    /// The paths in a tree, sorted.
    pub fn tree_paths(&self, revision: &str) -> Vec<String> {
        let listing = self.git(&["ls-tree", "-r", "--name-only", revision]);
        let mut paths: Vec<String> = listing.lines().map(str::to_string).collect();
        paths.sort();
        paths
    }

    /// `git status --porcelain`.
    pub fn status(&self) -> String {
        self.git(&["status", "--porcelain"])
    }

    /// A fingerprint of `HEAD`, the index and the worktree.
    ///
    /// The invariant `git tpl update` exists to protect is that none of the
    /// three moves. Comparing all three at once makes a violation impossible to
    /// miss and impossible to explain away.
    pub fn working_state(&self) -> WorkingState {
        WorkingState {
            head: self.try_git(&["rev-parse", "HEAD"]).1.trim().to_string(),
            index: self.git(&["write-tree"]),
            worktree: self.worktree_digest(),
        }
    }

    fn worktree_digest(&self) -> String {
        let mut entries = Vec::new();
        collect(&self.path, &self.path, &mut entries);
        entries.sort();
        entries.join("\n")
    }
}

/// `HEAD`, the index and the worktree, captured together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingState {
    pub head: String,
    pub index: String,
    pub worktree: String,
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if entry.file_name() == ".git" {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, out);
        } else if let Ok(bytes) = std::fs::read(&path) {
            use sha2::{Digest, Sha256};
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let digest = Sha256::digest(&bytes);
            out.push(format!(
                "{} {}",
                relative.to_string_lossy().replace('\\', "/"),
                hex::encode(digest)
            ));
        }
    }
}

/// Run `git-tpl` in a repository.
pub fn tpl(repo: &Repo, args: &[&str]) -> Output {
    let mut command = Command::cargo_bin("git-tpl").expect("built binary");
    command.current_dir(&repo.path);
    // Deterministic output, whatever the developer's terminal or CI is doing.
    command.arg("--color").arg("never");
    command.args(args);
    // A template repository is resolved relative to the project, and a stray
    // ambient config would change what the tests exercise.
    command.env_remove("NO_COLOR");
    command.env_remove("CLICOLOR_FORCE");

    let output = command.output().expect("run git-tpl");
    Output {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// What the binary produced.
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    /// Assert it exited zero, showing everything if not.
    pub fn success(self) -> Self {
        assert_eq!(
            self.code, 0,
            "expected success, got {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.code, self.stdout, self.stderr
        );
        self
    }

    /// Assert a specific exit code.
    pub fn code(self, expected: i32) -> Self {
        assert_eq!(
            self.code, expected,
            "expected exit {expected}, got {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.code, self.stdout, self.stderr
        );
        self
    }

    /// Assert it failed.
    pub fn failure(self) -> Self {
        assert_ne!(
            self.code, 0,
            "expected failure\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.stdout, self.stderr
        );
        self
    }

    /// Everything the command wrote.
    pub fn all(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }

    /// Assert some text appears in the output.
    pub fn says(self, needle: &str) -> Self {
        assert!(
            self.all().contains(needle),
            "expected to find {needle:?} in:\n{}",
            self.all()
        );
        self
    }

    /// Assert some text does not appear.
    pub fn silent_about(self, needle: &str) -> Self {
        assert!(
            !self.all().contains(needle),
            "did not expect {needle:?} in:\n{}",
            self.all()
        );
        self
    }
}

// --- template builders ------------------------------------------------------

/// A template repository under construction.
pub struct Template {
    pub repo: Repo,
}

impl Template {
    /// The template used by most tests: two questions, a computed value, a
    /// data source, a plain file and an executable.
    pub fn standard(parent: &Path) -> Self {
        let repo = Repo::init_in(parent, "template");
        repo.write(
            "template.toml",
            r#"
name = "rust-library"
description = "A small Rust library"

[data.licenses]
source = "data/licenses.toml"

[questions.project_name]
type = "string"
prompt = "Project name"
default = "demo"

[questions.license]
type = "choice"
prompt = "License"
choices_from = "data.licenses.ids"
default = "MIT"

[computed]
package_name = "{{ project_name | lower | replace(' ', '-') }}"
"#,
        );
        repo.write(
            "data/licenses.toml",
            r#"
ids = ["MIT", "Apache-2.0"]

[names]
MIT = "MIT License"
"Apache-2.0" = "Apache License 2.0"
"#,
        );
        repo.write(
            "template/Cargo.toml.jinja",
            "[package]\nname = \"{{ package_name }}\"\nversion = \"0.1.0\"\nlicense = \"{{ license }}\"\n",
        );
        repo.write(
            "template/README.md.jinja",
            "# {{ project_name }}\n\nLicensed under {{ data.licenses.names[license] }}.\n",
        );
        repo.write("template/src/lib.rs.jinja", "//! {{ project_name }}\n");
        // Deliberately not a `.jinja`: GitHub Actions files are full of
        // `${{ }}`, and a tool that rendered every file would mangle them.
        repo.write(
            "template/ci.yml",
            "name: CI\njobs:\n  test:\n    steps:\n      - run: echo ${{ github.sha }}\n",
        );
        repo.write("template/run.sh", "#!/bin/sh\necho run\n");
        repo.make_executable("template/run.sh");
        // The template's own README, which must NOT be rendered into projects.
        repo.write("README.md", "# The template's own README\n");
        repo.commit_all("feat: initial template");

        Self { repo }
    }

    /// A template with only a manifest and one file.
    pub fn minimal(parent: &Path, manifest: &str, files: &[(&str, &str)]) -> Self {
        let repo = Repo::init_in(parent, "template");
        repo.write("template.toml", manifest);
        for (path, content) in files {
            repo.write(&format!("template/{path}"), content);
        }
        repo.commit_all("feat: initial template");
        Self { repo }
    }

    /// The path a project should use to refer to this template.
    pub fn source(&self) -> String {
        self.repo.path.to_string_lossy().into_owned()
    }
}

/// A workspace holding a template and a project side by side.
pub struct World {
    pub dir: tempfile::TempDir,
    pub template: Template,
    pub project: Repo,
}

impl World {
    /// A world whose project already has a commit — the case that exercises the
    /// unrelated-histories merge.
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let template = Template::standard(dir.path());
        let project = Repo::init_in(dir.path(), "project");
        project.write("NOTES.md", "Pre-existing project notes.\n");
        project.commit_all("chore: initial commit");
        Self {
            dir,
            template,
            project,
        }
    }

    /// A world whose project has no commits at all.
    pub fn empty_project() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let template = Template::standard(dir.path());
        let project = Repo::init_in(dir.path(), "project");
        Self {
            dir,
            template,
            project,
        }
    }

    /// A world with a custom template.
    pub fn with_template(manifest: &str, files: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let template = Template::minimal(dir.path(), manifest, files);
        let project = Repo::init_in(dir.path(), "project");
        project.write("NOTES.md", "notes\n");
        project.commit_all("chore: initial commit");
        Self {
            dir,
            template,
            project,
        }
    }

    /// `git tpl init` against this world's template.
    pub fn init(&self, extra: &[&str]) -> Output {
        let source = self.template.source();
        let mut args = vec!["init", source.as_str(), "--defaults"];
        args.extend_from_slice(extra);
        tpl(&self.project, &args)
    }

    /// The rendered ref name for this world's template.
    pub fn ref_name(&self) -> String {
        "refs/tpl/template".to_string()
    }
}
