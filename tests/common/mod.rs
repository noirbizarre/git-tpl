//! Shared harness for the integration tests.
//!
//! Builds **real** Git repositories in temporary directories and drives the
//! real binary. Nothing about Git is mocked: the entire premise of the project
//! is that Git's behaviour is the behaviour, so a test against a stub would be
//! testing the stub.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use assert_cmd::prelude::*;

/// Detach a child process from the Git environment of the process that ran the
/// suite.
///
/// Under `git rebase --exec`, `git bisect run`, or any hook, Git exports
/// `GIT_DIR`, `GIT_WORK_TREE` and `GIT_INDEX_FILE`. They take precedence over
/// `current_dir`, so without this every `git` the harness spawns operates on
/// the git-tpl repository rather than on its own temporary directory: the
/// identity `Repo::configure` sets lands in the developer's own config, and
/// `LibGit2::init` clears `core.bare` from a worktree's config. Loud, wrong,
/// and it outlives the run.
///
/// The config sources are pinned for the same reason `XDG_CONFIG_HOME` is: a
/// test must not read — or be changed by — the developer's `~/.gitconfig`.
/// `config_home` only needs to name a directory; the file inside it is never
/// created, and Git treats a missing `GIT_CONFIG_GLOBAL` as empty.
pub fn scrub_git_env(command: &mut Command, config_home: &Path) {
    for name in [
        // Repository discovery: each one overrides `current_dir`.
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_COMMON_DIR",
        "GIT_CEILING_DIRECTORIES",
        "GIT_PREFIX",
        "GIT_NAMESPACE",
        // Identity: `rebase` and `commit` export these, and a test asserting on
        // authorship would silently pick up the ambient commit's identity
        // instead of the one `configure` set.
        "GIT_AUTHOR_NAME",
        "GIT_AUTHOR_EMAIL",
        "GIT_AUTHOR_DATE",
        "GIT_COMMITTER_NAME",
        "GIT_COMMITTER_EMAIL",
        "GIT_COMMITTER_DATE",
    ] {
        command.env_remove(name);
    }
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    command.env("GIT_CONFIG_GLOBAL", config_home.join("absent.gitconfig"));
}

/// Point `core.excludesFile` at a global ignore file holding `rules`.
///
/// Written to `<config_home>/git/config`, because that is where libgit2 looks
/// for a global config. It does *not* read `GIT_CONFIG_GLOBAL`, which is a
/// git-core environment variable — setting that alone would silently configure
/// nothing, and a test asserting that a global rule applied would pass by
/// accident.
///
/// The path is written with forward slashes. A backslash begins an escape
/// sequence in Git's config syntax, so a raw Windows path spells `\U` and
/// `\R` at libgit2, which rejects the whole file — and a repository whose
/// global config will not parse cannot be opened at all. Forward slashes are
/// accepted on Windows and cost nothing elsewhere.
pub fn global_gitignore(config_home: &Path, rules: &str) {
    let ignore = config_home.join("global.gitignore");
    std::fs::write(&ignore, rules).expect("write global ignore");

    let git = config_home.join("git");
    std::fs::create_dir_all(&git).expect("create git config dir");
    std::fs::write(
        git.join("config"),
        format!(
            "[core]\n\texcludesFile = {}\n",
            ignore.display().to_string().replace('\\', "/")
        ),
    )
    .expect("write global config");
}

/// A Git repository under test.
pub struct Repo {
    pub path: PathBuf,
    /// Kept so a repository that owns its temporary directory outlives it.
    /// `None` when the directory belongs to a [`World`].
    pub _dir: Option<tempfile::TempDir>,
    /// An `$XDG_CONFIG_HOME` of this repository's own, empty unless a test
    /// fills it.
    ///
    /// Every `tpl()` invocation points at it, so the suite can never read —
    /// or be changed by — the developer's `~/.config/git-tpl/config.toml`.
    /// Outside the worktree, or `status` and `diff` would see it.
    pub _config_dir: tempfile::TempDir,
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
            _config_dir: tempfile::tempdir().expect("config home"),
        };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.configure();
        repo
    }

    /// A repository inside an existing directory, sharing its lifetime.
    pub fn init_in(parent: &Path, name: &str) -> Self {
        let path = parent.join(name);
        std::fs::create_dir_all(&path).expect("create repo dir");
        let repo = Self {
            path,
            _dir: None,
            _config_dir: tempfile::tempdir().expect("config home"),
        };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.configure();
        repo
    }

    /// The configuration every test repository needs, in one place so the two
    /// constructors cannot drift apart. Set per-repository rather than
    /// globally so the tests cannot depend on — or disturb — the developer's
    /// own config.
    fn configure(&self) {
        // libgit2 refuses to build a signature without an identity, and a
        // fresh CI runner has none.
        self.git(&["config", "user.name", "Test"]);
        self.git(&["config", "user.email", "test@example.invalid"]);
        self.git(&["config", "commit.gpgsign", "false"]);
        // Windows runners ship `core.autocrlf=true` globally. Rendering is
        // deterministic, but the checkout that materialises the merge applies
        // the repository's line-ending filters, so an inherited `autocrlf`
        // rewrites LF to CRLF on the way into the worktree and every assertion
        // comparing a rendered file against an LF literal fails — on Windows
        // and nowhere else. Pin the repository rather than teach those
        // assertions about the host's Git configuration.
        self.git(&["config", "core.autocrlf", "false"]);
        self.git(&["config", "core.eol", "lf"]);
    }

    /// An existing repository at a path, with its own isolated config home.
    ///
    /// For a repository this harness did not create — a bare remote pushed to
    /// by a test, say.
    pub fn at(path: PathBuf) -> Self {
        Self {
            path,
            _dir: None,
            _config_dir: tempfile::tempdir().expect("config home"),
        }
    }

    /// This repository's isolated `$XDG_CONFIG_HOME`.
    pub fn config_home(&self) -> &Path {
        self._config_dir.path()
    }

    /// Write the user configuration `tpl()` will read.
    pub fn user_config(&self, toml: &str) -> &Self {
        let path = self.config_home().join("git-tpl").join("config.toml");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create config dir");
        std::fs::write(&path, toml).expect("write user config");
        self
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

    /// Make a file executable, in the index as well as on disk.
    ///
    /// The index half is what makes this work off Unix, where there is no bit
    /// on disk to set. Without it a fixture is committed `100644` on Windows
    /// and every test about the executable bit has to be `#[cfg(unix)]` — which
    /// hides the thing actually worth pinning, that a *committed* `100755`
    /// renders to `100755` on every platform.
    ///
    /// `commit_all`'s later `git add -A` does not undo it: Windows runners have
    /// `core.fileMode=false`, so Git keeps the index mode of a file whose
    /// content has not changed.
    pub fn make_executable(&self, relative: &str) -> &Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = self.path.join(relative);
            let mut permissions = std::fs::metadata(&path).expect("stat").permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&path, permissions).expect("chmod");
        }
        // `update-index` can only speak about a tracked path, so stage it first.
        self.git(&["add", "--", relative]);
        self.git(&["update-index", "--chmod=+x", "--", relative]);
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
        let mut command = Command::new("git");
        command.args(args).current_dir(&self.path);
        scrub_git_env(&mut command, self.config_home());
        let output = command.output().expect("run git");
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
        let mut command = Command::new("git");
        command.args(args).current_dir(&self.path);
        scrub_git_env(&mut command, self.config_home());
        let output = command.output().expect("run git");
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

    /// The Git mode of one path in a tree, e.g. `100755`.
    ///
    /// Asserted on in preference to the filesystem: the mode that matters is
    /// the one recorded in the tree, because that is what a second machine
    /// compares against when deciding whether `git tpl update` has anything to
    /// commit.
    pub fn file_mode(&self, revision: &str, path: &str) -> String {
        let line = self.git(&["ls-tree", revision, "--", path]);
        line.split_whitespace()
            .next()
            .filter(|mode| !mode.is_empty())
            .unwrap_or_else(|| panic!("no entry for `{path}` in `{revision}`"))
            .to_string()
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
/// A `file://` URL for a local path, spelled the way libgit2 parses one.
///
/// Not `format!("file://{}", path.display())`: on Windows that yields
/// `file://C:\Users\...`, where `C:` is read as the authority and the
/// backslashes are not separators, and libgit2 rejects it outright. A drive
/// path needs the empty authority and forward slashes — `file:///C:/Users/...`.
///
/// One form serves both, without a branch that would be dead on whichever
/// platform is running: a POSIX path's leading `/` and a drive letter's lack of
/// one are the only difference, so drop it and put it back.
pub fn file_url(path: &Path) -> String {
    let path = path.display().to_string().replace('\\', "/");
    format!("file:///{}", path.trim_start_matches('/'))
}

pub fn tpl(repo: &Repo, args: &[&str]) -> Output {
    run_tpl(&repo.path, repo.config_home(), args, "never")
}

/// The same, but with colour forced on.
///
/// A test cannot simply pass `--color always`, because clap refuses the flag
/// twice and [`tpl`] has already supplied it. Needed by anything asserting on
/// styled output — without it the branch that *chooses* to style is never
/// taken, and could be inverted without failing a single test.
pub fn tpl_colored(repo: &Repo, args: &[&str]) -> Output {
    run_tpl(&repo.path, repo.config_home(), args, "always")
}

/// Run `git-tpl` in a plain directory, with no repository.
///
/// The project-free commands — `render`, `lint`, `questions`, `context` — must
/// work here, and a test that ran them inside a repository would not prove it.
pub fn tpl_outside(dir: &Path, config_home: &Path, args: &[&str]) -> Output {
    run_tpl(dir, config_home, args, "never")
}

fn run_tpl(cwd: &Path, config_home: &Path, args: &[&str], color: &str) -> Output {
    let mut command = Command::cargo_bin("git-tpl").expect("built binary");
    command.current_dir(cwd);
    // Deterministic output, whatever the developer's terminal or CI is doing.
    // Explicit rather than inherited, so `always` means the same on a CI
    // runner with no tty as it does in a terminal.
    command.arg("--color").arg(color);
    command.args(args);
    // A template repository is resolved relative to the project, and a stray
    // ambient config would change what the tests exercise.
    command.env_remove("NO_COLOR");
    command.env_remove("CLICOLOR_FORCE");
    // The user configuration is read from `$XDG_CONFIG_HOME/git-tpl/`, so
    // without this the suite would read whatever the developer happens to have
    // written there — and `[defaults]` would change what a prompt returns.
    // `HOME` goes too, since it is the fallback the resolution uses.
    command.env("XDG_CONFIG_HOME", config_home);
    command.env_remove("HOME");
    // The binary opens the repository at `cwd`; an inherited `GIT_DIR` from a
    // `git rebase --exec` or a hook would point it somewhere else entirely.
    scrub_git_env(&mut command, config_home);

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
    /// The JSON on stdout.
    ///
    /// Panics with the whole output when it is not JSON, because the usual
    /// cause is a human line that escaped onto stdout — which is the bug
    /// `--json` exists to prevent, and a `serde` error alone would not name it.
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|error| {
            panic!(
                "stdout is not JSON ({error})\n--- stdout ---\n{}\n--- stderr ---\n{}",
                self.stdout, self.stderr
            )
        })
    }

    /// The diagnostic code of a failure envelope.
    ///
    /// Tests assert on this rather than on the message: the codes are the
    /// stable surface, and pinning prose is how error messages stop improving.
    pub fn error_code(&self) -> String {
        self.json()["error"]["code"]
            .as_str()
            .unwrap_or_else(|| panic!("no error code in {}", self.stdout))
            .to_string()
    }

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

    /// The whole run as a reviewable block: the exit code, then each stream
    /// under its own heading.
    ///
    /// `all()` concatenates the two streams, and the boundary between them is
    /// exactly what the snapshots exist to pin: `--json` owns stdout and human
    /// prose owns stderr, so a snapshot that could not tell them apart would
    /// keep passing while that broke.
    ///
    /// Stdout is pretty-printed when it parses as JSON. The binary emits the
    /// compact form on purpose (`report::success` calls `Value::to_string`),
    /// and `tests/json.rs` still pins that; the expansion here is so a reviewer
    /// can see which key changed rather than diffing one very long line.
    ///
    /// An empty stream is written as `<empty>` rather than left blank, because
    /// insta trims trailing whitespace and would otherwise absorb an empty
    /// stream turning into a stray newline.
    pub fn transcript(&self) -> String {
        format!(
            "exit: {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.code,
            section(&pretty_json(&self.stdout)),
            section(&self.stderr),
        )
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

/// A stream as it appears in a transcript.
fn section(stream: &str) -> String {
    if stream.is_empty() {
        "<empty>".to_string()
    } else {
        stream.trim_end_matches('\n').to_string()
    }
}

/// Expand JSON for review, and leave anything else exactly as it was.
fn pretty_json(stdout: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(stdout) {
        Ok(value) => serde_json::to_string_pretty(&value).expect("re-serialise JSON"),
        Err(_) => stdout.to_string(),
    }
}

/// Snapshot settings for the object ids.
///
/// Paths are *not* filtered here — see `redact_paths`, which has to rewrite
/// what it matches rather than merely replace it, and so cannot be expressed as
/// an `insta` filter.
pub fn snapshot_settings() -> insta::Settings {
    let mut settings = insta::Settings::clone_current();

    // Full object ids first: the short rule below would otherwise consume the
    // leading seven characters of one and leave a fragment behind.
    settings.add_filter(r"\b[0-9a-f]{40}\b", "[sha]");
    // `Oid::short` is `to_hex()[..7]`, and it reaches the prose through
    // `ops::describe_revision` — `main (7fa834c)` and the `X → Y` form.
    settings.add_filter(r"\b[0-9a-f]{7,8}\b", "[short]");

    // The `Date:` header of a `git tpl backport` mailbox, which is the one
    // thing this project prints that is different on every run. Without this
    // the snapshot passes on the day it is taken and never again.
    settings.add_filter(
        r"\w{3}, \d{1,2} \w{3} \d{4} \d{2}:\d{2}:\d{2} \+0000",
        "[date]",
    );

    // One file, one snapshot per test: the module prefix would repeat
    // `snapshots__` in every name without distinguishing anything.
    settings.set_prepend_module_to_snapshot(false);

    settings
}

/// The temporary directory of this world, in every form the binary might have
/// resolved it to.
///
/// macOS resolves `/var/folders/...` to `/private/var/folders/...`, and Windows
/// resolves the 8.3 short name `RUNNER~1` to `runneradmin` — in both cases the
/// path printed is not the path `TempDir` handed us. `canonicalize` gives the
/// resolved form back with a `\\?\` verbatim prefix nothing else ever prints,
/// so it goes.
fn temporary_directories(world: &World) -> Vec<String> {
    let root = world.dir.path().to_path_buf();
    let canonical = root.canonicalize().unwrap_or_else(|_| root.clone());

    [&root, &canonical]
        .into_iter()
        .map(|path| {
            let text = path.display().to_string();
            text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
        })
        .collect()
}

/// Replace this world's temporary directory with `[tmp]`, and write whatever
/// follows it with `/` separators.
///
/// Not an `insta` filter, because a filter can only substitute what it matched
/// and this has to *rewrite* it: on Windows the tail is `\template` in prose
/// and `\\template` inside a JSON string, and a snapshot that recorded either
/// would fail everywhere else.
pub fn redact_paths(world: &World, text: &str) -> String {
    redact_roots(&temporary_directories(world), text)
}

/// The substitution itself, over roots supplied by the caller.
///
/// Split out from `redact_paths` so it can be tested against the macOS and
/// Windows spellings from a Linux machine — every bug this function exists to
/// fix was found by CI on a platform the author could not run.
pub fn redact_roots(roots: &[String], text: &str) -> String {
    // Each root reaches the output in three spellings: as given, with `/`
    // separators, and `\\`-escaped inside a JSON string.
    let mut spellings: Vec<String> = roots
        .iter()
        .flat_map(|root| {
            [
                root.replace('\\', "/"),
                root.replace('\\', r"\\"),
                root.clone(),
            ]
        })
        .collect();
    // Longest first. `/var/folders/...` is a suffix of `/private/var/...`, so
    // matching the short one first would redact the middle of the long one and
    // leave `/private[tmp]` behind — which is what CI caught.
    spellings.sort_by_key(|spelling| std::cmp::Reverse(spelling.len()));
    spellings.dedup();

    let alternation = spellings
        .iter()
        .map(|spelling| regex::escape(spelling))
        .collect::<Vec<_>>()
        .join("|");
    // The tail is bounded by a character class rather than by `.*`, so a
    // redaction stops at the quote or backtick a message wraps the path in.
    let pattern = format!(r"(?i)(?:{alternation})((?:(?:\\{{1,2}}|/)[\w.@~+-]+)*)");
    let paths = regex::Regex::new(&pattern).expect("a path pattern");

    paths
        .replace_all(text, |captured: &regex::Captures| {
            format!(
                "[tmp]{}",
                captured[1].replace(r"\\", "/").replace('\\', "/")
            )
        })
        .into_owned()
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
        Self::with_shared(parent, manifest, files, &[])
    }

    /// The same, plus files written *outside* the render root.
    ///
    /// `files` are relative to `template/` and get rendered; `shared` are
    /// relative to the repository root and do not. That distinction is the
    /// whole of the partial rule, so the harness makes it explicit rather than
    /// leaving each test to spell out a `../`.
    pub fn with_shared(
        parent: &Path,
        manifest: &str,
        files: &[(&str, &str)],
        shared: &[(&str, &str)],
    ) -> Self {
        let repo = Repo::init_in(parent, "template");
        repo.write("template.toml", manifest);
        for (path, content) in files {
            repo.write(&format!("template/{path}"), content);
        }
        for (path, content) in shared {
            repo.write(path, content);
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
        Self::with_shared_template(manifest, files, &[])
    }

    /// A world with a custom template that also has files outside the render
    /// root — the partials an `{% import %}` may resolve to.
    pub fn with_shared_template(
        manifest: &str,
        files: &[(&str, &str)],
        shared: &[(&str, &str)],
    ) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let template = Template::with_shared(dir.path(), manifest, files, shared);
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

    /// Advance the template by one commit: `README.md` gains a line at the
    /// end, and a new file appears.
    ///
    /// Both a modification and an addition, so that a caller can assert on the
    /// two change kinds without setting up its own template move.
    pub fn move_template(&self) {
        self.template.repo.write(
            "template/README.md.jinja",
            "# {{ project_name }}\n\nLicensed under {{ data.licenses.names[license] }}.\n\nGenerated by git-tpl.\n",
        );
        self.template
            .repo
            .write("template/.github/workflows/release.yml", "name: Release\n");
        self.template
            .repo
            .commit_all("feat: add a release workflow");
    }

    /// The rendered ref name for this world's template.
    pub fn ref_name(&self) -> String {
        "refs/tpl/template".to_string()
    }
}

// --- a real HTTP server -----------------------------------------------------

/// A minimal HTTP/1.1 server, for the remote data source tests.
///
/// Hand-rolled rather than `wiremock` or `httpmock`, both of which pull in an
/// async runtime this project does not otherwise have. It also matches the rest
/// of this harness: real Git repositories, and now a real socket, because a
/// test against a stubbed transport tests the stub.
pub struct TestServer {
    base: String,
    routes: Arc<Vec<(String, u16, Vec<u8>)>>,
    hits: Arc<AtomicUsize>,
    shutdown: Arc<AtomicBool>,
}

impl TestServer {
    /// Serve a fixed set of `path -> (status, body)` routes.
    ///
    /// Bound on port 0, so tests run concurrently without agreeing a port.
    pub fn start(routes: Vec<(&str, u16, Vec<u8>)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        // Non-blocking so the accept loop can notice the shutdown flag instead
        // of parking forever on a port nobody will connect to again.
        listener.set_nonblocking(true).expect("non-blocking");

        let routes: Arc<Vec<(String, u16, Vec<u8>)>> = Arc::new(
            routes
                .into_iter()
                .map(|(p, s, b)| (p.to_string(), s, b))
                .collect(),
        );
        let hits = Arc::new(AtomicUsize::new(0));
        let shutdown = Arc::new(AtomicBool::new(false));

        let server = Self {
            base: format!("http://127.0.0.1:{port}"),
            routes: Arc::clone(&routes),
            hits: Arc::clone(&hits),
            shutdown: Arc::clone(&shutdown),
        };

        std::thread::spawn(move || {
            while !shutdown.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        // BSD/macOS propagate the listener's O_NONBLOCK to the
                        // accepted socket; Linux does not. Left set, a body
                        // larger than the send buffer is truncated by a
                        // WouldBlock, and the size-limit test sees a short
                        // response instead of an oversized one.
                        stream.set_nonblocking(false).expect("blocking stream");
                        // A client that dies mid-request must not park a worker
                        // thread until nextest's terminate-after kills the run.
                        stream
                            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                            .expect("read timeout");
                        let routes = Arc::clone(&routes);
                        let hits = Arc::clone(&hits);
                        std::thread::spawn(move || serve_one(stream, &routes, &hits));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        server
    }

    /// The server's origin, for a template that builds its own URL.
    pub fn base_url(&self) -> String {
        self.base.clone()
    }

    /// The absolute URL of a served path.
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// How many requests have been served.
    ///
    /// The whole point of the "fetched at most once per run" guarantee is that
    /// this number stays at 1 however many questions use the source.
    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

fn serve_one(mut stream: TcpStream, routes: &[(String, u16, Vec<u8>)], hits: &AtomicUsize) {
    // Only the request line is needed, and it is the first thing on the wire.
    // Headers are read and discarded; there is no request body to worry about
    // because the client only ever issues GETs.
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) => break,
            Ok(_) if header.trim().is_empty() => break,
            Ok(_) => {}
            Err(_) => return,
        }
    }

    let path = request_line.split_whitespace().nth(1).unwrap_or("/");
    hits.fetch_add(1, Ordering::Relaxed);

    let (status, body) = routes
        .iter()
        .find(|(p, _, _)| p == path)
        .map(|(_, s, b)| (*s, b.clone()))
        .unwrap_or((404, b"not found".to_vec()));

    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Status",
    };

    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    write_response(&mut stream, head.as_bytes());
    write_response(&mut stream, &body);
    let _ = stream.flush();
}

/// Write a whole response part, or panic.
///
/// Only a client that hung up is tolerated: the size-limit test deliberately
/// aborts once it has read enough. Every other error means the response reached
/// the client truncated, which a test would otherwise read as a legitimately
/// short body — the exact way the macOS non-blocking bug hid itself.
fn write_response(stream: &mut TcpStream, bytes: &[u8]) {
    match stream.write_all(bytes) {
        Ok(()) => {}
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
            ) => {}
        Err(e) => panic!("test server failed to write a response: {e}"),
    }
}
