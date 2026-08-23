//! Orchestration — one operation per command.
//!
//! Everything below this module is unaware that commands exist. These
//! functions compose resolution, evaluation, rendering and Git into the
//! operations the CLI exposes.

pub mod backport;
pub mod hunks;
pub mod resolve;
pub mod testing;
pub mod unsubstitute;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use miette::Diagnostic;
use thiserror::Error;

use crate::config::{CONFIG_PATH, Config, ConfigError};
use crate::context::Context;
use crate::data::{
    AlwaysTrust, DataError, Decided, Decision, Loader, REMOTE_LIMIT_BYTES, RefuseRemote,
    TemplateTree, TrustGate, declared_remotes,
};
use crate::eval::{DefaultsOnly, EvalError, Evaluation, Partials, Prompter};
use crate::git::{AheadBehind, Change, FileStat, GitBackend, GitError, MergeOutcome, Oid};
use crate::gitconfig::{Preferences, push_refspec, seed};
use crate::graph::{Graph, GraphError};
use crate::provenance::{Provenance, Recorded};
use crate::refs::{TemplateId, TemplateIdError};
use crate::render::{RenderError, Rendered, render_entries, write_tree};
use crate::seed::SeedContext;
use crate::template::{MANIFEST_NAME, Manifest, Value};
use crate::userconfig::UserConfig;

pub use resolve::{Request, ResolveError, Resolved};

pub use crate::migration::{self, Migration, MigrationError, Move};
pub use backport::{Backport, BackportError, BackportedFile, Skipped, backport};
pub use hunks::{Hunk, Picker, Picking, Selection};
pub use unsubstitute::{Proposal, Unsubstitute, Unsubstituter, Unsubstitution, Verdict};

/// Errors from any operation.
#[derive(Debug, Error, Diagnostic)]
pub enum OpError {
    /// The project configuration is missing or invalid.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Config(#[from] ConfigError),

    /// The user's own configuration is invalid.
    ///
    /// Separate from `Config` because the two files have different owners: this
    /// one is on the machine running the command, and no amount of editing the
    /// project will fix it.
    #[error(transparent)]
    #[diagnostic(transparent)]
    UserConfig(#[from] crate::userconfig::UserConfigError),

    /// The template could not be resolved.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Resolve(#[from] ResolveError),

    /// The template's dependency graph is invalid.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Graph(#[from] GraphError),

    /// Evaluation failed.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Eval(#[from] EvalError),

    /// An `--answers-from` file could not be read.
    ///
    /// Separate from `InvalidArgument` because the flag itself was well formed:
    /// what failed was the file it named, and the diagnostic that says so
    /// already carries the path and the reason.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Answers(#[from] crate::answers::AnswersError),

    /// A data source was refused, or could not be confirmed.
    ///
    /// Separate from `Eval` because the trust gate runs before evaluation
    /// starts, so this failure has no question to attach itself to.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Data(#[from] DataError),

    /// Rendering failed.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Render(#[from] RenderError),

    /// A template failed its static checks.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Lint(#[from] crate::lint::LintError),

    /// The test runner could not run.
    ///
    /// Only failures that stop the run reach here. An unmet expectation is a
    /// [`testing::Failure`] carried in the report, not an error: twelve failing
    /// cases must all be reported, and an error would report one.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Test(#[from] testing::TestError),

    /// A backport could not be produced.
    ///
    /// Always a refusal, never a wrong patch: a backport that guesses ships a
    /// broken template to every downstream project at once. See ADR-020.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Backport(#[from] BackportError),

    /// A Git operation failed.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Git(#[from] GitError),

    /// A migration could not be parsed or applied.
    ///
    /// Its own variant rather than folded into `Render` or `Git`: a migration
    /// is neither — it is discovered from a tree diff and applied to one, but
    /// the failure a user needs to act on is about the migration file itself.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Migration(#[from] MigrationError),

    /// The template id could not be determined.
    #[error(transparent)]
    #[diagnostic(transparent)]
    TemplateId(#[from] TemplateIdError),

    /// A `note_file` names nothing at the template revision.
    ///
    /// Fatal rather than silent, and it can be: the note is resolved before the
    /// ref is created and before the merge, so nothing has been written to the
    /// user's repository yet. Showing nothing instead would leave a template
    /// author with an `init` that succeeds and a note that never appears.
    #[error("`{path}` is not in the template at {revision_description}")]
    #[diagnostic(
        code(tpl::ops::missing_note_file),
        help(
            "`note_file` is relative to the template repository root, not to the \
             render root — a note beside the manifest is `NEXT-STEPS.md`, not \
             `template/NEXT-STEPS.md`. `git tpl lint` reports this without a \
             repository."
        )
    )]
    MissingNoteFile {
        /// The path, as it resolved.
        path: String,
        /// The revision it was looked for at, as `reference (revision)`.
        //
        // `*_description`, not `revision`: the naming rule reserves `revision`
        // for an `Oid`, and this is the printable pair `describe_revision`
        // produces.
        revision_description: String,
    },

    /// A `note_file` is not valid UTF-8.
    ///
    /// Refused rather than decoded lossily, for the same reason a binary
    /// partial is: replacement characters would look like something was shown.
    #[error("`{path}` is not valid UTF-8")]
    #[diagnostic(
        code(tpl::ops::note_file_not_utf8),
        help("a note is text; this path names a binary file")
    )]
    NoteFileNotUtf8 {
        /// The path, as it resolved.
        path: String,
    },

    /// A migration file's own content is not valid UTF-8.
    #[error("`{path}` is not valid UTF-8")]
    #[diagnostic(
        code(tpl::ops::migration_file_not_utf8),
        help("a migration file is TOML text; this path names a binary file")
    )]
    MigrationFileNotUtf8 {
        /// The migration file's path.
        path: String,
    },

    /// A migration's `message_file` names nothing at the template revision.
    ///
    /// Fatal rather than silent, for the same reason `MissingNoteFile` is:
    /// discovered and resolved before the ref moves, so nothing has been
    /// committed yet.
    #[error("`{path}` is not in the template at {revision_description}")]
    #[diagnostic(
        code(tpl::ops::missing_migration_message_file),
        help(
            "`message_file` in a migration is relative to the template repository \
             root, not to the render root. `git tpl lint` reports this without a \
             repository."
        )
    )]
    MissingMigrationMessageFile {
        /// The migration file that declared it.
        migration: String,
        /// The `message_file` path, as it resolved.
        path: String,
        /// The revision it was looked for at, as `reference (revision)`.
        revision_description: String,
    },

    /// A migration's `message_file` is not valid UTF-8.
    #[error("`{path}` is not valid UTF-8")]
    #[diagnostic(
        code(tpl::ops::migration_message_file_not_utf8),
        help("a message is text; this path names a binary file")
    )]
    MigrationMessageFileNotUtf8 {
        /// The migration file that declared it.
        migration: String,
        /// The `message_file` path, as it resolved.
        path: String,
    },

    /// `init` was run on a project that already has a template.
    #[error("this repository already has a template attached")]
    #[diagnostic(
        code(tpl::ops::already_initialised),
        help(
            "`{CONFIG_PATH}` already exists. Run `git tpl update` to re-render, \
             or edit the answers there and update."
        )
    )]
    AlreadyInitialised,

    /// A command-line argument was not usable.
    ///
    /// Its own variant rather than a `GitError`: a malformed `--answer` has
    /// nothing to do with Git, and dressing it as one sends the reader looking
    /// in the wrong place.
    #[error("{message}")]
    #[diagnostic(code(tpl::ops::invalid_argument))]
    InvalidArgument {
        /// What was wrong.
        message: String,
    },

    /// The destination `init` was pointed at does not exist.
    ///
    /// Its own variant rather than `InvalidArgument`: the argument was well
    /// formed, and the fix — `--init`, or create the directory yourself — is
    /// worth naming rather than folded into a generic message.
    #[error("`{}` does not exist", path.display())]
    #[diagnostic(
        code(tpl::ops::no_such_directory),
        help("create it first, or pass --init to create it and the repository")
    )]
    NoSuchDirectory {
        /// The destination that was asked for.
        path: PathBuf,
    },

    /// The rendered ref does not exist.
    #[error("`{ref_name}` does not exist")]
    #[diagnostic(
        code(tpl::ops::no_rendered_ref),
        help("run `git tpl update` to render it, or `git tpl fetch` if it is shared")
    )]
    NoRenderedRef {
        /// The ref that was looked for.
        ref_name: String,
    },

    /// The path is not in the rendering.
    ///
    /// Both fields are carried because the two things the reader does not
    /// already know are which path was looked for *after normalisation* and
    /// which ref was read.
    #[error("`{path}` is not in `{ref_name}`")]
    #[diagnostic(
        code(tpl::ops::no_such_path),
        help("run `git tpl diff --name-only` to list what the template renders")
    )]
    NoSuchPath {
        /// The path that was looked for.
        path: String,
        /// The ref it was looked for in.
        ref_name: String,
    },

    /// Generated output could not be written to disk.
    ///
    /// Its own variant rather than a `ConfigError`: nothing about the project
    /// is at fault. Somebody asked for a file in a directory they cannot write,
    /// and the two things they do not already know are which path was attempted
    /// and what the operating system said about it.
    #[error("could not write `{path}`")]
    #[diagnostic(
        code(tpl::ops::write_failed),
        help("reason: {reason}\ncheck that the directory exists and is writable")
    )]
    WriteFailed {
        /// The path that could not be written.
        path: String,
        /// What the operating system reported.
        reason: String,
    },
}

/// How answers are obtained during an operation.
pub enum Answering<'a> {
    /// Prompt for anything unanswered.
    Interactive(&'a mut dyn Prompter),
    /// Take defaults; a question with no default is an error.
    ///
    /// Owns its prompter rather than borrowing a shared one, so that
    /// `Answering::Defaults` can be constructed without the caller having to
    /// keep a `DefaultsOnly` alive alongside it.
    Defaults(DefaultsOnly),
}

impl Answering<'_> {
    /// Take defaults for every unanswered question.
    pub fn defaults() -> Self {
        Answering::Defaults(DefaultsOnly)
    }

    /// Whether a human is going to be asked anything.
    ///
    /// Read before any prompt seed is fetched, so a non-interactive run does
    /// not so much as read the machine's Git configuration.
    fn is_interactive(&self) -> bool {
        matches!(self, Answering::Interactive(_))
    }

    fn prompter(&mut self) -> &mut dyn Prompter {
        match self {
            Answering::Interactive(prompter) => *prompter,
            Answering::Defaults(defaults) => defaults,
        }
    }
}

/// How a revision is written in output: the name asked for, plus the commit it
/// resolved to.
///
/// A branch name alone cannot tell you whether the template moved, which is the
/// question every one of these lines exists to answer.
pub fn describe_revision(reference: &str, commit: Oid) -> String {
    if reference == crate::provenance::WORKTREE_REF {
        format!("{} (+ uncommitted changes)", commit.short())
    } else {
        format!("{reference} ({})", commit.short())
    }
}

/// Make `dir` exist and be empty.
///
/// Cleared rather than merged into, because a template that stops producing a
/// file must be seen to stop: writing over a previous run would leave the old
/// file behind, and the author would conclude the conditional works.
///
/// `failed` names the caller's own diagnostic, because "could not write your
/// output directory" and "could not write a snapshot" are different failures
/// to the person reading them.
pub fn clear_directory<E>(
    dir: &Path,
    failed: &impl Fn(&Path, &str, &std::io::Error) -> E,
) -> Result<(), E> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(failed(dir, "clear", &error)),
    }
    std::fs::create_dir_all(dir).map_err(|error| failed(dir, "create", &error))
}

/// Write a rendered tree onto the filesystem under `root`.
///
/// The one place that turns rendered bytes into files, so that the executable
/// bit is applied identically wherever a tree lands on disk. It had two
/// implementations and they had already diverged: one set the bit and never
/// cleared it, so a non-executable file written over an executable one kept a
/// mode the rendering does not have.
///
/// Safe to join `root` with each path: every rendered path was validated by
/// `render_path`, which rejects `.`, `..`, absolute segments and separators
/// inside a segment. Joining is safe because of that check, not in spite of it.
pub fn materialise<'a, E>(
    root: &Path,
    files: impl IntoIterator<Item = (&'a str, &'a [u8], bool)>,
    failed: &impl Fn(&Path, &str, &std::io::Error) -> E,
) -> Result<(), E> {
    for (path, content, executable) in files {
        let target = root.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| failed(parent, "create", &error))?;
        }
        std::fs::write(&target, content).map_err(|error| failed(&target, "write", &error))?;
        set_executable(&target, executable)
            .map_err(|error| failed(&target, "set the permissions of", &error))?;
    }
    Ok(())
}

/// Apply the executable bit, on the platforms that have one.
///
/// Git records nothing else about permissions, so this is the whole of what a
/// materialised tree has to reproduce. On Windows there is no bit to set,
/// which is why a snapshot records the mode separately.
fn set_executable(path: &Path, executable: bool) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)?.permissions();
        let mode = permissions.mode();
        // Cleared as well as set. Only ever setting it means a file that stops
        // being executable keeps a mode the rendering does not have.
        permissions.set_mode(if executable {
            mode | 0o111
        } else {
            mode & !0o111
        });
        std::fs::set_permissions(path, permissions)?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, executable);
    }
    Ok(())
}

/// How a template's remote data sources are authorised.
///
/// Per invocation, and nothing is recorded: the next run asks again. A
/// persistent trust list is a user-side fact and belongs in a user
/// configuration file, not in the project — a project cannot consent on its
/// reader's behalf.
pub enum Trust<'a> {
    /// Confirm each source with the user before any is fetched.
    Ask(&'a mut dyn TrustGate),
    /// Allow everything without asking. `--trust`.
    Always(AlwaysTrust),
    /// Refuse everything, because there is nobody to ask.
    Refuse(RefuseRemote),
    /// Replay a decision already taken earlier in this invocation.
    ///
    /// For `git tpl test`, which renders one template many times: the consent
    /// is sought once, before the first case, and every case then honours it —
    /// including the sources the user refused.
    Decided(Decided),
}

impl Trust<'_> {
    /// Allow every remote source for this run.
    pub fn always() -> Self {
        Trust::Always(AlwaysTrust)
    }

    /// Refuse every remote source, loudly, at the point of use.
    pub fn refuse() -> Self {
        Trust::Refuse(RefuseRemote)
    }

    /// Honour a decision taken earlier in this invocation.
    pub fn decided(decisions: BTreeMap<String, Decision>) -> Self {
        Trust::Decided(Decided::new(decisions))
    }

    fn gate(&mut self) -> &mut dyn TrustGate {
        match self {
            Trust::Ask(gate) => *gate,
            Trust::Always(always) => always,
            Trust::Refuse(refuse) => refuse,
            Trust::Decided(decided) => decided,
        }
    }
}

/// A rendered state, as bytes, before any Git object exists.
///
/// This is what a project-free render produces: `git tpl render --output` and
/// `git tpl lint` need the files and the context, and have no repository to
/// write blobs into. [`Render`] is this plus the tree.
pub struct RenderedFiles {
    /// The resolved template.
    pub template: Resolved,
    /// The resolved context.
    pub context: Context,
    /// The rendered files, sorted by output path.
    pub files: Vec<Rendered>,
    /// What produced them.
    pub provenance: Provenance,
    /// Supplied answers that name no question in this template.
    pub ignored_answers: Vec<String>,
}

/// A rendered state, before it is committed.
pub struct Render {
    /// The resolved template.
    pub template: Resolved,
    /// The resolved context.
    pub context: Context,
    /// The rendered tree.
    pub tree: Oid,
    /// What produced it.
    pub provenance: Provenance,
    /// Supplied answers that name no question in this template.
    ///
    /// Reported rather than fatal, and reported rather than silent — see the
    /// comment where it is computed.
    pub ignored_answers: Vec<String>,
}

/// What a rendering needs to know about the template it is rendering.
///
/// A struct rather than loose arguments here, because unlike the decisions in
/// [`render_files`] these four travel together: they are one template
/// reference, and a caller that has one has all of them.
pub struct Target<'a> {
    /// Where the template comes from.
    pub source: &'a str,
    /// The branch, tag or SHA to render, or `None` for the default branch.
    pub reference: Option<&'a str>,
    /// A render root overriding the manifest's, or `None`.
    pub root: Option<&'a str>,
    /// Render the template's working tree rather than a committed revision.
    pub dirty: bool,
}

impl<'a> Target<'a> {
    /// The template a project has recorded.
    pub fn from_config(config: &'a Config, dirty: bool) -> Self {
        Self {
            source: &config.template.source,
            reference: config.template.r#ref.as_deref(),
            root: config.template.root.as_deref(),
            dirty,
        }
    }
}

/// A rendering, against a template somebody else resolved.
///
/// [`RenderedFiles`] is this plus the [`Resolved`] that produced it. The two
/// are split so that a caller with many answer sets — [`testing::run`] — can
/// resolve once: [`resolve::resolve`] makes a fresh temporary clone every call,
/// so a resolution per answer set costs a clone each and, worse, could render
/// two answer sets against two different revisions if the branch moved between
/// them.
pub struct RenderedOnce {
    /// The resolved context.
    pub context: Context,
    /// The rendered files, sorted by output path.
    pub files: Vec<Rendered>,
    /// What produced them.
    pub provenance: Provenance,
    /// Supplied answers that name no question in this template.
    pub ignored_answers: Vec<String>,
}

/// Resolve, evaluate and render — to bytes, with no repository required.
///
/// `project` is `None` for a project-free render. Two things depend on it, and
/// both degrade honestly rather than guessing: prompt seeds are not collected
/// (there is no `git config` to read, and nobody to prompt), and a `local` data
/// source becomes [`DataError::NeedsProject`](crate::data::DataError::NeedsProject)
/// rather than being resolved against the process's working directory.
///
/// Everything short of writing a Git object happens here, so `render` and a
/// project-free render cannot come to disagree about what a rendering is.
// Every argument is a distinct decision the caller has already made, and
// bundling them into a struct would only move the list somewhere a reader has
// to go and find it.
#[allow(clippy::too_many_arguments)]
pub fn render_files(
    target: Target<'_>,
    project: Option<(&dyn GitBackend, &Path)>,
    supplied: BTreeMap<String, Value>,
    user: &UserConfig,
    answering: Answering<'_>,
    trust: Trust<'_>,
) -> Result<RenderedFiles, OpError> {
    let template = resolve::resolve(Request {
        source: target.source,
        reference: target.reference,
        root: target.root,
        dirty: target.dirty,
    })?;

    let rendered = render_resolved(
        &template,
        target.source,
        project,
        supplied,
        user,
        answering,
        trust,
    )?;

    Ok(RenderedFiles {
        template,
        context: rendered.context,
        files: rendered.files,
        provenance: rendered.provenance,
        ignored_answers: rendered.ignored_answers,
    })
}

/// Evaluate and render an already-resolved template.
///
/// The body of [`render_files`] minus the resolution, so that a caller holding
/// one [`Resolved`] can render it repeatedly against different answer sets
/// without re-cloning and without risking two revisions in one run.
#[allow(clippy::too_many_arguments)]
pub fn render_resolved(
    template: &Resolved,
    source: &str,
    project: Option<(&dyn GitBackend, &Path)>,
    supplied: BTreeMap<String, Value>,
    user: &UserConfig,
    mut answering: Answering<'_>,
    trust: Trust<'_>,
) -> Result<RenderedOnce, OpError> {
    // Built and validated before anything is prompted: a cycle or a typo
    // discovered after six answered questions is the worst possible time.
    let graph = Graph::build(&template.manifest)?;

    // A supplied answer naming no question is ignored rather than fatal: an
    // answers file carried over from another generator has `_src_path` and
    // `_commit` in it, and a template drops questions over time — erroring
    // would make `--answers-from` useless for the case that motivated it.
    // Silence would swallow a typo, so it is reported instead. The caller
    // prints it; this layer does not print.
    let ignored_answers: Vec<String> = supplied
        .keys()
        .filter(|key| !template.manifest.questions.contains_key(*key))
        .cloned()
        .collect();

    // Every remote source is confirmed here, before evaluation, so the user
    // sees the whole of what the template wants to do on the network in one
    // place. Loading is lazy and interleaved with the questionnaire, so asking
    // at fetch time would scatter network prompts through the questions.
    //
    // A template with no remote data is never asked about — the overwhelming
    // majority, and they must not acquire a prompt they have no use for.
    let requests = declared_remotes(&template.manifest.data);

    // A `[trust]` entry is prior consent, deliberately written, and no weaker
    // than `--trust` — so it grants even when there is nobody to ask. Refusing
    // it non-interactively would leave `--trust` on every CI invocation as the
    // only way to use a template you have already agreed to, which teaches
    // people to pass `--trust` unconditionally.
    //
    // Applied here rather than where `Trust` is constructed, because that is in
    // the CLI and only this layer has the source: `update` reads it out of the
    // project configuration. One place, so `init`, `update` and both dry runs
    // cannot disagree.
    //
    // An *unmatched* template is untouched, and `Trust::Refuse` still refuses
    // it loudly. Nothing is granted by omission.
    let mut trust = if user.trust.allows(source) {
        Trust::always()
    } else {
        trust
    };

    let decisions: BTreeMap<String, Decision> = if requests.is_empty() {
        BTreeMap::new()
    } else {
        trust.gate().confirm(&requests, REMOTE_LIMIT_BYTES)?
    };

    let mut loader = Loader::new(
        TemplateTree {
            repo: template.repo.as_ref(),
            tree: template.tree,
            revision: template.revision,
        },
        project.map(|(_, root)| root.to_path_buf()),
    )
    .with_decisions(decisions);

    // Built only when somebody is going to be asked. When nobody is, the map
    // is empty *and* `DefaultsOnly` ignores it — two guards, because a machine
    // value reaching the tree would end invariant 2. A derived seed is still
    // just a seed: widening what a `default_from` may read does not widen where
    // the result may go, and both guards stay exactly here.
    let seeds = match project {
        Some((repo, _)) if answering.is_interactive() => {
            prompt_seeds(repo, &template.manifest, user)?
        }
        _ => BTreeMap::new(),
    };

    // Read once, up front, and shared by the manifest expressions below and the
    // file rendering further down — one environment, one set of importable
    // names, so a macro usable in a `.jinja` file is usable in a `computed`.
    let partials = template.partials()?;

    let context = crate::eval::resolve(
        Evaluation {
            manifest: &template.manifest,
            graph: &graph,
            supplied,
            seeds: &seeds,
            partials: &partials,
        },
        &mut loader,
        answering.prompter(),
    )?;

    let entries = template.entries()?;
    // Bytes, not blobs. Turning them into Git objects is `render`'s job, and it
    // is the only part of a rendering that needs a repository to write into.
    // `strict` is the template's own choice. Lenient is still the default, so
    // that turning it on is a decision an author makes rather than one an
    // upgrade makes for them; `git tpl lint` reports the same names as
    // warnings meanwhile. See ADR-014.
    let undefined = if template.manifest.strict.unwrap_or(false) {
        crate::eval::Undefined::Strict
    } else {
        crate::eval::Undefined::Lenient
    };
    let files = render_entries(
        template.repo.as_ref(),
        &entries,
        &context,
        &partials,
        undefined,
    )?;

    let provenance = Provenance {
        source: source.to_string(),
        reference: template.reference.clone(),
        commit: template.revision,
        dirty: template.dirty,
        answers_digest: context.answers_digest(),
        data: loader.provenance().to_vec(),
        version: crate::VERSION.to_string(),
        template_name: template.manifest.name.clone(),
    };

    Ok(RenderedOnce {
        context,
        files,
        provenance,
        ignored_answers,
    })
}

/// Resolve, evaluate and render — everything short of touching a ref.
///
/// Shared by `init`, `update` and `--dry-run`, so all three cannot disagree
/// about what a rendering is. A thin wrapper over [`render_files`]: the only
/// thing it adds is writing the bytes into the project as Git objects.
#[allow(clippy::too_many_arguments)]
pub fn render(
    project: &dyn GitBackend,
    project_root: &Path,
    config: &Config,
    supplied: BTreeMap<String, Value>,
    dirty: bool,
    user: &UserConfig,
    answering: Answering<'_>,
    trust: Trust<'_>,
) -> Result<Render, OpError> {
    let rendered = render_files(
        Target::from_config(config, dirty),
        Some((project, project_root)),
        supplied,
        user,
        answering,
        trust,
    )?;

    // Blobs are read from the template repository — often a temporary clone —
    // and written into the project, which is where the ref will point.
    let tree = write_tree(project, &rendered.files)?;

    Ok(Render {
        template: rendered.template,
        context: rendered.context,
        tree,
        provenance: rendered.provenance,
        ignored_answers: rendered.ignored_answers,
    })
}

/// Collect everything a prompt may be pre-filled with, in precedence order.
///
/// Two sources, and the user's own wins:
///
/// ```text
/// [defaults] in the user configuration  >  default_from  >  the question's default
/// ```
///
/// A `default_from` is the *template author's* suggestion about where an answer
/// usually comes from; `[defaults]` is the person at the keyboard stating it
/// outright. When both speak, the person does.
///
/// A key that names no question — or names one of another type — is skipped in
/// silence, unlike an ignored `--answers-from` key. The difference is the file:
/// an answers file is supplied for *this* template, so a key it does not
/// recognise is a typo, whereas `[defaults]` is written once for every template
/// the user will ever generate and is *expected* to overshoot. Reporting
/// `author` on every run of every template that has no `author` question would
/// be noise, and noise is how a real warning stops being read.
///
/// A source that is simply unset is absent: a template suggesting `user.name`
/// must still work for someone who has never set one, and the question's own
/// `default` covers that. The same goes for an expression rendering to nothing.
fn prompt_seeds(
    project: &dyn GitBackend,
    manifest: &Manifest,
    user: &UserConfig,
) -> Result<BTreeMap<String, Value>, OpError> {
    let mut seeds = BTreeMap::new();

    // Built at most once, and only if some question actually asks for it. A
    // template with no expression seed must not pay for a configuration
    // snapshot and a remote lookup.
    let mut machine: Option<SeedContext> = None;

    for (name, question) in &manifest.questions {
        // The shorthand keeps its own path: no engine, no parse, and exactly
        // the behaviour it had before expressions existed.
        if let Some(key) = question.git_config_key() {
            if let Some(value) = seed(project, key)? {
                seeds.insert(name.clone(), Value::String(value));
            }
            continue;
        }

        let Some(expression) = question.default_from_expression() else {
            continue;
        };

        let machine = match machine {
            Some(ref built) => built,
            None => {
                // `tpl.remote` and not `--remote`: a flag about where template
                // refs are pushed must not silently change a prompt default.
                let remote = Preferences::load(project)?.remote;
                machine.insert(crate::seed::collect(project, &remote)?)
            }
        };

        let rendered = crate::eval::render_seed(
            expression,
            machine,
            &format!("questions.{name}.default_from"),
        )?;

        // An expression resolving to nothing is an absent seed, not an empty
        // prompt — the same rule `gitconfig::seed` applies to an unset key.
        if !rendered.trim().is_empty() {
            seeds.insert(name.clone(), Value::String(rendered));
        }
    }

    apply_user_defaults(&mut seeds, manifest, user);

    Ok(seeds)
}

/// Overlay the user's `[defaults]` onto the seeds a manifest asked for.
///
/// Split out from [`prompt_seeds`] so the precedence rule can be tested without
/// a repository — it is the part with a decision in it.
fn apply_user_defaults(
    seeds: &mut BTreeMap<String, Value>,
    manifest: &Manifest,
    user: &UserConfig,
) {
    for (name, value) in &user.defaults {
        let Some(question) = manifest.questions.get(name) else {
            continue;
        };
        // Type-checked rather than coerced. A seed that does not fit is a
        // collision with an unrelated template's question of the same name, and
        // pre-filling a boolean prompt with a string would be worse than not
        // pre-filling it at all.
        if question.kind.accepts(value) {
            seeds.insert(name.clone(), value.clone());
        }
    }
}

/// What happened to one declared remote.
///
/// A template declares Git remotes under `[remotes]`; git-tpl adds them on
/// `init` and never fetches or pushes them. See ADR-019.
#[derive(Debug, Clone, PartialEq)]
pub enum RemoteOutcome {
    /// It was not configured, and now is.
    Added,
    /// It was already configured with this URL, so nothing was done.
    Unchanged,
    /// It was configured with a *different* URL and was left alone.
    ///
    /// Never overwritten. A template that could repoint an existing `origin` is
    /// a template that could redirect the user's next push, and the URL in the
    /// repository was put there by a person.
    Skipped {
        /// The URL the repository already had.
        existing: String,
    },
}

/// One declared remote and what became of it.
///
/// Not `Remote`: [`crate::remote::Remote`] is a *parsed remote URL*, taken
/// apart to seed a prompt. This is a template's declaration and its fate, which
/// is a different thing, and two types called `Remote` would be one concept
/// wearing two meanings.
#[derive(Debug, Clone, PartialEq)]
pub struct DeclaredRemote {
    /// The remote's name, as declared.
    pub name: String,
    /// The URL the template asked for, with its expression evaluated.
    pub url: String,
    /// What happened.
    pub outcome: RemoteOutcome,
}

/// The result of an `init`.
pub struct InitOutcome {
    /// The template id, and so the ref name.
    pub id: TemplateId,
    /// The commit created on the rendered ref.
    pub commit: Oid,
    /// What was rendered.
    pub changes: Vec<Change>,
    /// How the merge into the branch went, if one was attempted.
    pub merge: Option<MergeOutcome>,
    /// Where the configuration was written.
    pub config_path: PathBuf,
    /// Whether it was committed, or left staged for the user's merge
    /// resolution.
    pub config_committed: bool,
    /// The revision that was rendered, ready to print.
    pub revision_description: String,
    /// Supplied answers that name no question in this template.
    pub ignored_answers: Vec<String>,
    /// Template files a `.gitignore` removed from the rendering.
    ///
    /// Only ever non-empty under `--dirty`, and surfaced rather than silent:
    /// a global `core.excludesFile` rule the author forgot they wrote is
    /// otherwise invisible, since there is no `git status` inside a render.
    pub ignored: Vec<String>,
    /// The template's own note to the user, raw and unsanitised.
    ///
    /// Sanitised where it is shown, not here: this layer does not print, and a
    /// `--json` consumer is not a terminal and must get the text as written.
    /// See [`crate::note`].
    pub note: Option<String>,
    /// The declared remotes, in declaration order, and what became of each.
    pub remotes: Vec<DeclaredRemote>,
}

/// Attach a template to a repository and merge the initial rendering.
///
/// The merge is the load-bearing step: without it the template commit is not an
/// ancestor of the branch, so the *first* `update` would have no merge base and
/// Git could not tell the user's edits from the template's — every file that
/// differs would conflict, including ones the user customised that the template
/// never changed. See `docs/adr/009-init-merges-unrelated-histories.md`, and
/// `tests/init.rs::without_a_merge_base_a_customisation_conflicts_the_template_never_touched`
/// for the demonstration.
#[allow(clippy::too_many_arguments)]
pub fn init(
    project: &dyn GitBackend,
    project_root: &Path,
    source: &str,
    reference: Option<String>,
    explicit_id: Option<&str>,
    supplied: BTreeMap<String, Value>,
    dirty: bool,
    merge_after: bool,
    force: bool,
    user: &UserConfig,
    answering: Answering<'_>,
    trust: Trust<'_>,
) -> Result<InitOutcome, OpError> {
    // `--force` re-asks the questions and renders onto the existing ref, which
    // is not a new operation: the ref is append-only, so another rendering on
    // it is exactly what `update` writes. The only thing `force` adds is
    // asking again, which `update` has no way to do.
    if !force && Config::exists_in(project_root) {
        return Err(OpError::AlreadyInitialised);
    }

    // A merge needs a clean worktree, and so does this. Checked before doing
    // any work rather than after the questionnaire.
    if merge_after && !project.is_empty()? && !project.is_clean()? {
        return Err(GitError::DirtyWorktree.into());
    }

    let mut config = Config::new(source, reference);
    config.template.id = explicit_id.map(str::to_string);

    let rendered = render(
        project,
        project_root,
        &config,
        supplied,
        dirty,
        user,
        answering,
        trust,
    )?;

    let id = TemplateId::resolve(source, explicit_id)?;
    let ref_name = id.ref_name();

    // Before the ref, before the configuration, before the merge. A `note_file`
    // that names nothing is an authoring mistake, and this is the last moment
    // at which saying so costs the user nothing: after the next line there is a
    // commit in their repository to explain away.
    let note = template_note(&rendered)?;

    // An orphan commit: the template has no history in this project before
    // now, and inventing a parent would be a lie.
    let commit = project.create_commit(rendered.tree, &[], &rendered.provenance.to_message())?;
    project.set_ref(&ref_name, commit, "tpl: initial render")?;

    let changes = project.diff_trees(None, rendered.tree, &[])?;

    // Written after rendering, so a template that fails to render leaves no
    // half-initialised project behind.
    config.answers = rendered.context.answers().clone();
    let config_path = config.save(project_root)?;

    // `.config/git.tpl.toml` is versioned with the project — a fresh clone must
    // be understandable from it alone. Leaving it untracked would mean the
    // template attachment existed only on the machine that ran `init`.
    //
    // It rides in the merge commit rather than in one of its own: the merge is
    // the attachment, and a `chore(tpl): attach` commit on top of it said
    // nothing the merge did not. See ADR-021.
    let merge = if merge_after {
        let outcome = project.merge(
            commit,
            &format!(
                "Merge template {} into {}\n\n\
                 Initial rendering of the template attached by `git tpl init`.\n\
                 Records the template source and the answers used to render it.\n\
                 See {CONFIG_PATH}.\n",
                rendered.template.manifest.name,
                project
                    .head_branch()?
                    .unwrap_or_else(|| "the branch".into())
            ),
            true,
            &[Path::new(CONFIG_PATH)],
        )?;
        Some(outcome)
    } else {
        None
    };

    let config_committed = match &merge {
        // The merge commit carries it.
        Some(MergeOutcome::Merged { .. }) => true,

        // Conflicts are the user's to resolve, and their resolution commit is
        // where the configuration belongs. Committing now would make a commit
        // in the middle of a merge they have not finished.
        Some(MergeOutcome::Conflicted { .. }) => {
            project.stage(Path::new(CONFIG_PATH))?;
            false
        }

        // No merge commit exists to carry it: an empty repository
        // fast-forwards to the render commit (ADR-009), and `--no-merge` asked
        // for no merge at all. The render commit itself cannot hold the
        // configuration — it is the ref tip, and must stay byte-identical to
        // the rendering, or an unchanged template would stop producing no
        // commit.
        //
        // Staged after the merge, not before: a dirty index makes libgit2
        // refuse to merge, and the failure would be about the index rather
        // than about anything the user did.
        _ => {
            project.stage(Path::new(CONFIG_PATH))?;
            project.commit_index(&format!(
                "chore(tpl): attach the {} template\n\n\
                 Records the template source and the answers used to render it.\n\
                 See {CONFIG_PATH}.\n",
                rendered.template.manifest.name
            ))?;
            true
        }
    };

    // After the ref, the merge and the configuration, so a template's own
    // additions cannot get between the user and a rendering that already
    // succeeded. `init`-only: `update` being a ref-only operation is most of
    // its value. See ADR-019.
    //
    // The note is resolved much earlier, above — it can fail, and this is past
    // the point where failing is free.
    let remotes = add_remotes(project, &rendered)?;

    Ok(InitOutcome {
        id,
        commit,
        changes,
        merge,
        config_path,
        config_committed,
        revision_description: describe_revision(
            &rendered.template.reference,
            rendered.template.revision,
        ),
        ignored_answers: rendered.ignored_answers,
        ignored: rendered.template.ignored,
        note,
        remotes,
    })
}

/// The template's note to the user, with its expression evaluated.
///
/// Evaluated here rather than in `render.rs`: nothing about a note reaches the
/// tree, and running it through the renderer would put a value that is never
/// written to a file inside the code path invariant 2 guards.
///
/// Called *before* the ref is created and before the merge, which is what makes
/// a missing file an error rather than a shrug. While the note was read out of
/// the rendered tree it could only be resolved after the merge, and failing an
/// `init` that had already written to the user's repository would have been a
/// worse outcome than showing nothing.
fn template_note(rendered: &Render) -> Result<Option<String>, OpError> {
    let manifest = &rendered.template.manifest;
    if manifest.note.is_none() && manifest.note_file.is_none() {
        return Ok(None);
    }

    let partials = rendered.template.partials()?;

    let Some(declared) = &manifest.note_file else {
        return manifest
            .note
            .as_ref()
            .map(|text| crate::eval::render_string(text, &rendered.context, "note", &partials))
            .transpose()
            .map_err(Into::into);
    };

    let path = crate::eval::render_string(declared, &rendered.context, "note_file", &partials)?;
    let path = path.trim();

    // A path that renders to nothing is a template choosing to say nothing for
    // these answers — `note_file = "{% if ci %}notes/ci.md{% endif %}"`. That is
    // a decision, not a mistake, and is the one absence not worth reporting.
    if path.is_empty() {
        return Ok(None);
    }

    // Repository-root-relative, read from the whole template tree rather than
    // the rendered subtree — the same namespace partials live in. The note is
    // read from the template and never written into the project.
    let Some(bytes) = rendered
        .template
        .repo
        .read_path(rendered.template.tree, path)?
    else {
        return Err(OpError::MissingNoteFile {
            path: path.to_string(),
            revision_description: describe_revision(
                &rendered.template.reference,
                rendered.template.revision,
            ),
        });
    };

    // Not `from_utf8_lossy`: a binary note is an authoring mistake, and printing
    // replacement characters would hide it behind something that looks shown.
    let text = String::from_utf8(bytes).map_err(|_| OpError::NoteFileNotUtf8 {
        path: path.to_string(),
    })?;

    // Rendered if and only if it is a template, which is the rule the renderer
    // applies to files. Nothing is inferred from the content: an author who
    // wants interpolation names the `.jinja`.
    if path.ends_with(crate::render::TEMPLATE_SUFFIX) {
        return crate::eval::render_string(&text, &rendered.context, path, &partials)
            .map(Some)
            .map_err(Into::into);
    }

    Ok(Some(text))
}

/// A migration's message, with its expression evaluated.
///
/// The other half of [`template_note`], and resolved exactly the same way —
/// `message_file` is repository-root-relative, read from the whole template
/// tree, rendered only if it ends in `.jinja`. [`crate::migration::parse`]
/// only validates the *shape* of `message`/`message_file`; evaluating either
/// against a project's answers needs the [`Context`] this function has and
/// [`crate::migration`] deliberately does not.
fn migration_message(
    migration: &Migration,
    rendered: &Render,
    partials: &std::sync::Arc<Partials>,
) -> Result<Option<String>, OpError> {
    let Some(declared) = &migration.message_file else {
        return migration
            .message
            .as_ref()
            .map(|text| {
                crate::eval::render_string(text, &rendered.context, &migration.path, partials)
            })
            .transpose()
            .map_err(Into::into);
    };

    let path = crate::eval::render_string(declared, &rendered.context, &migration.path, partials)?;
    let path = path.trim();

    // A path that renders to nothing is the migration choosing to say
    // nothing for these answers — the same reading `template_note` gives
    // `note_file`.
    if path.is_empty() {
        return Ok(None);
    }

    let Some(bytes) = rendered
        .template
        .repo
        .read_path(rendered.template.tree, path)?
    else {
        return Err(OpError::MissingMigrationMessageFile {
            migration: migration.path.clone(),
            path: path.to_string(),
            revision_description: describe_revision(
                &rendered.template.reference,
                rendered.template.revision,
            ),
        });
    };

    let text = String::from_utf8(bytes).map_err(|_| OpError::MigrationMessageFileNotUtf8 {
        migration: migration.path.clone(),
        path: path.to_string(),
    })?;

    if path.ends_with(crate::render::TEMPLATE_SUFFIX) {
        return crate::eval::render_string(&text, &rendered.context, path, partials)
            .map(Some)
            .map_err(Into::into);
    }

    Ok(Some(text))
}

/// Add the remotes a template declares, reporting what happened to each.
///
/// Never fetches and never pushes — ADR-019's closure rule admits the addition
/// and nothing beyond it.
fn add_remotes(
    project: &dyn GitBackend,
    rendered: &Render,
) -> Result<Vec<DeclaredRemote>, OpError> {
    let declared = &rendered.template.manifest.remotes;
    if declared.is_empty() {
        return Ok(Vec::new());
    }

    let partials = rendered.template.partials()?;
    let mut remotes = Vec::with_capacity(declared.len());

    for (name, url) in declared {
        let url = crate::eval::render_string(
            url,
            &rendered.context,
            &format!("remotes.{name}"),
            &partials,
        )?;

        let outcome = match project.remote_url(name)? {
            // Already exactly this. Reported as unchanged rather than added, so
            // a second `init --force` does not claim to have done something.
            Some(existing) if existing == url => RemoteOutcome::Unchanged,
            // Left alone, loudly. Overwriting would let a template redirect a
            // push the user is about to make.
            Some(existing) => RemoteOutcome::Skipped { existing },
            // Absent — or present with a URL that is not UTF-8, which
            // `remote_url` cannot tell apart from absent. The add settles it.
            None => match project.add_remote(name, &url) {
                Ok(()) => RemoteOutcome::Added,
                // There after all. Left alone, exactly as a readable one would
                // have been; the URL cannot be shown because it is not text.
                Err(GitError::RemoteExists { .. }) => RemoteOutcome::Skipped {
                    existing: "(not valid UTF-8)".to_string(),
                },
                Err(error) => return Err(error.into()),
            },
        };

        remotes.push(DeclaredRemote {
            name: name.clone(),
            url,
            outcome,
        });
    }

    Ok(remotes)
}

/// One migration discovered and applied by an `update`.
///
/// The message is raw and unsanitised, in `InitOutcome::note`'s tradition:
/// this layer does not print, and a `--json` consumer is not a terminal and
/// must get the text as written. Sanitised where it is shown — see
/// [`crate::note`].
pub struct AppliedMigration {
    /// The migration file's path, repository-root-relative.
    pub path: String,
    /// The migration's message, if it declared one.
    pub message: Option<String>,
    /// The paths it moved, if any.
    pub moves: Vec<migration::Move>,
}

/// The result of an `update`.
pub enum UpdateOutcome {
    /// The rendered tree was identical to the ref's tip; nothing was committed.
    ///
    /// The reason determinism matters: a renderer that varied would create a
    /// commit on every run, and every one would be noise to merge. Never the
    /// outcome when a migration was newly discovered — see `Updated`.
    UpToDate {
        /// The revision that was rendered, ready to print.
        revision_description: String,
        /// Supplied answers that name no question in this template. Carried
        /// even here: a typo'd key is worth reporting whether or not the
        /// rendering changed.
        ignored_answers: Vec<String>,
        /// Template files a `.gitignore` removed from the rendering.
        ///
        /// Only ever non-empty under `--dirty`, and surfaced rather than silent:
        /// a global `core.excludesFile` rule the author forgot they wrote is
        /// otherwise invisible, since there is no `git status` inside a render.
        ignored: Vec<String>,
    },
    /// A new commit was added to the rendered ref.
    Updated {
        /// The template id.
        id: TemplateId,
        /// The new commit.
        commit: Oid,
        /// What changed against the previous rendering.
        changes: Vec<Change>,
        /// The revision previously rendered, if there was one, ready to print.
        previous_revision_description: Option<String>,
        /// The revision now rendered, ready to print.
        revision_description: String,
        /// Whether the recorded answers were rewritten — which happens when a
        /// template adds a question. Worth telling the user, since it is the
        /// one file `update` does modify.
        answers_changed: bool,
        /// Whether the commit was written onto an empty ref, and so starts a
        /// history unrelated to anything the branch has merged.
        ///
        /// Two causes, both legitimate and neither obvious: the configuration's
        /// `source` or `id` was edited, so the ref name changed; or the project
        /// was cloned without `refs/tpl/*` and never fetched. Either way the
        /// next `git tpl merge` has no merge base and can conflict on every
        /// file, which is worth saying before it happens.
        started_new_history: bool,
        /// Migrations newly crossed by this update, in application order.
        ///
        /// Empty on every ordinary update — a migration is discovered exactly
        /// once, at whichever update first crosses it. See docs/adr/024.
        migrations: Vec<AppliedMigration>,
        /// The intermediate, content-identical rename commit, when one was
        /// needed to make a move's rename reliably detectable by a plain
        /// `git merge`.
        ///
        /// `None` on almost every update, including most that carry a
        /// migration: only a move that lands alongside some other content
        /// change to the same rendering needs it. See docs/adr/024.
        moved_commit: Option<Oid>,
        /// Supplied answers that name no question in this template.
        ignored_answers: Vec<String>,
        /// Template files a `.gitignore` removed from the rendering.
        ///
        /// Only ever non-empty under `--dirty`, and surfaced rather than silent:
        /// a global `core.excludesFile` rule the author forgot they wrote is
        /// otherwise invisible, since there is no `git status` inside a render.
        ignored: Vec<String>,
    },
}

/// Re-render and advance the rendered ref.
///
/// Never touches `HEAD`, the index or the worktree. That is structural: the
/// tree is built as a Git object and one ref is moved. There is no code path
/// here that writes a file into the project.
pub fn update(
    project: &dyn GitBackend,
    project_root: &Path,
    overrides: BTreeMap<String, Value>,
    dirty: bool,
    user: &UserConfig,
    answering: Answering<'_>,
    trust: Trust<'_>,
) -> Result<UpdateOutcome, OpError> {
    let mut config = Config::load(project_root)?;

    // Recorded answers first, then command-line overrides. A question added to
    // the template since the last render has no recorded answer and is
    // prompted for.
    let mut supplied = config.answers.clone();
    supplied.extend(overrides);

    let rendered = render(
        project,
        project_root,
        &config,
        supplied,
        dirty,
        user,
        answering,
        trust,
    )?;

    let id = TemplateId::resolve(&config.template.source, config.template.id.as_deref())?;
    let ref_name = id.ref_name();
    let tip = project.resolve_ref(&ref_name)?;

    let previous = tip.map(|oid| project.commit(oid)).transpose()?;
    let recorded_previous = previous
        .as_ref()
        .and_then(|commit| Provenance::parse(&commit.message));
    let previous_revision_description = recorded_previous.as_ref().map(Recorded::describe_revision);

    // Migrations newly crossed since the last render. `recorded.commit` is the
    // `Template-Commit` trailer of the previous rendered commit — read back
    // here for the first time rather than only for display, and the whole
    // reason no template ever needs to declare a version: the template's own
    // history between that commit and the one just resolved *is* the version
    // boundary. No previous commit, or one with no parseable provenance (a
    // hand-made commit on the ref, or the very first render): there is no
    // coherent "old state" to migrate away from, so migrations are skipped
    // rather than firing every migration the template has ever had.
    let mut migrations: Vec<AppliedMigration> = Vec::new();
    if let (Some(_), Some(old_commit)) = (&previous, recorded_previous.and_then(|r| r.commit)) {
        let old_tree = rendered.template.repo.commit_tree(old_commit)?;
        let new_tree = rendered.template.tree;
        if old_tree != new_tree {
            let partials = rendered.template.partials()?;
            for (path, bytes) in
                migration::discover_new(rendered.template.repo.as_ref(), old_tree, new_tree)?
            {
                let text = String::from_utf8(bytes)
                    .map_err(|_| OpError::MigrationFileNotUtf8 { path: path.clone() })?;
                let parsed = migration::parse(&text, &path)?;
                let message = migration_message(&parsed, &rendered, &partials)?;
                migrations.push(AppliedMigration {
                    path,
                    message,
                    moves: parsed.moves,
                });
            }
        }
    }

    // Every newly discovered migration's moves, in file order, applied
    // against the ref's current tip — never against `rendered.tree`, which
    // has no notion of where a path used to be.
    let all_moves: Vec<migration::Move> = migrations
        .iter()
        .flat_map(|m| m.moves.iter().cloned())
        .collect();
    let moved_tree = match &previous {
        Some(previous) => migration::apply_moves(project, previous.tree, &all_moves)?,
        None => None,
    };

    // Identical output, and nothing newly crossed. Committing would add a
    // commit that changes nothing, which the user would then have to merge
    // for no reason. This is what the determinism guarantee buys. A migration
    // bypasses it deliberately: without a commit here, the provenance trailer
    // never advances past it, and the same migration would surface again on
    // every later update — the one piece of state this design has no other
    // way to avoid needing.
    let identical = previous.as_ref().is_some_and(|p| p.tree == rendered.tree);
    if migrations.is_empty() && identical {
        return Ok(UpdateOutcome::UpToDate {
            revision_description: describe_revision(
                &rendered.template.reference,
                rendered.template.revision,
            ),
            ignored_answers: rendered.ignored_answers,
            ignored: rendered.template.ignored,
        });
    }

    // A move that fully explains the difference between the two renderings
    // needs no intermediate commit: the final commit built below already *is*
    // the pure rename. One is only inserted when a move lands alongside some
    // other content change in the same update — the case a plain `git merge`'s
    // similarity heuristic could otherwise miss. See docs/adr/024.
    const MOVE_COMMIT_MESSAGE: &str = "tpl: apply migration moves\n\n\
         A content-identical rename, so that the merge that follows \
         attributes it correctly rather than seeing an unrelated \
         delete and add. Superseded immediately by the next commit.";

    let mut moved_commit = None;
    let parents: Vec<Oid> = match (&previous, moved_tree) {
        (Some(previous), Some(moved)) if moved != rendered.tree => {
            let commit = project.create_commit(moved, &[previous.oid], MOVE_COMMIT_MESSAGE)?;
            moved_commit = Some(commit);
            vec![commit]
        }
        // Append-only. The parent is the current tip, whatever the reason for
        // re-rendering — template moved, answer changed, data changed.
        // Rewriting would destroy the merge base the branch already shares
        // with the ref. See docs/adr/005-append-only-refs.md.
        _ => tip.into_iter().collect(),
    };
    // No tip to descend from: this rendering shares no ancestry with whatever
    // the branch merged before. Not an error — a fresh clone has no
    // `refs/tpl/*` until it fetches — but the caller must say so.
    let started_new_history = parents.is_empty();
    let commit =
        project.create_commit(rendered.tree, &parents, &rendered.provenance.to_message())?;
    project.set_ref(&ref_name, commit, "tpl: update")?;

    let changes = project.diff_trees(previous.as_ref().map(|c| c.tree), rendered.tree, &[])?;

    // Answers are written back so that a question the template just added is
    // recorded rather than asked again on every update.
    //
    // Deliberately NOT staged or committed. `update` touching the index would
    // break the guarantee the whole design rests on — that it changes one ref
    // and nothing else. When the answers change, the user sees the edit in
    // `git status` and commits it with whatever else they are doing.
    let answers_changed = config.answers != *rendered.context.answers();
    config.answers = rendered.context.answers().clone();
    config.save(project_root)?;

    Ok(UpdateOutcome::Updated {
        id,
        commit,
        changes,
        previous_revision_description,
        revision_description: describe_revision(
            &rendered.template.reference,
            rendered.template.revision,
        ),
        answers_changed,
        started_new_history,
        migrations,
        moved_commit,
        ignored_answers: rendered.ignored_answers,
        ignored: rendered.template.ignored,
    })
}

/// Everything `status` reports.
pub struct Status {
    /// The template source.
    pub source: String,
    /// The template id.
    pub id: TemplateId,
    /// The rendered ref name.
    pub ref_name: String,
    /// The ref's tip, if it exists.
    pub tip: Option<Oid>,
    /// What the last rendering recorded.
    pub recorded: Option<Recorded>,
    /// What the configured `ref` resolves to now.
    pub available_revision_description: Option<String>,
    /// Whether the template has moved since the last rendering.
    pub template_moved: bool,
    /// Whether the ref's tip is an ancestor of `HEAD`.
    pub merged: bool,
    /// How the local ref compares to the remote copy, if there is one.
    pub remote: Option<(String, AheadBehind)>,
    /// Whether the worktree is clean.
    pub worktree_clean: bool,
    /// How many renderings the ref holds.
    pub rendering_count: usize,
}

impl Status {
    /// Whether anything is pending — the template moved, or a rendering is
    /// unmerged. Drives the exit code, so `git tpl status --quiet` is usable
    /// in CI as a drift check.
    pub fn is_pending(&self) -> bool {
        self.template_moved || (self.tip.is_some() && !self.merged)
    }
}

/// Report the state of the template attachment.
pub fn status(
    project: &dyn GitBackend,
    project_root: &Path,
    preferences: &Preferences,
    dirty: bool,
) -> Result<Status, OpError> {
    let config = Config::load(project_root)?;
    let id = TemplateId::resolve(&config.template.source, config.template.id.as_deref())?;
    let ref_name = id.ref_name();

    let tip = project.resolve_ref(&ref_name)?;
    let recorded = match tip {
        Some(oid) => Provenance::parse(&project.commit(oid)?.message),
        None => None,
    };

    // Resolving the template is a network operation, so a failure here is
    // reported as "unknown" rather than aborting the whole status. Being
    // offline should not stop you learning what is attached.
    //
    // `dirty` compares against the template's working tree instead of its
    // committed revision, which is how an author asks "does my uncommitted
    // edit change anything here?" without committing it first.
    let resolved = resolve::resolve(Request {
        source: &config.template.source,
        reference: config.template.r#ref.as_deref(),
        root: config.template.root.as_deref(),
        dirty,
    })
    .ok();

    let available_revision_description = resolved
        .as_ref()
        .map(|r| describe_revision(&r.reference, r.revision));

    let template_moved = match (&resolved, &recorded) {
        // A `--dirty` resolution reports the *base* commit, so comparing
        // revisions would say "unmoved" whenever the working tree sits on the
        // rendered commit — which is the common case and the one the flag was
        // asked about. The honest answer is that an uncommitted template is
        // always something to re-render, because nothing recorded what it
        // contained.
        (Some(resolved), _) if resolved.dirty => true,
        (Some(resolved), Some(recorded)) => recorded
            .commit
            .is_some_and(|commit| commit != resolved.revision),
        // Nothing rendered yet, but a template resolves: there is work to do.
        (Some(_), None) => tip.is_none(),
        _ => false,
    };

    let head = project.head_commit()?;
    let merged = match (tip, head) {
        (Some(tip), Some(head)) => project.is_ancestor(tip, head)?,
        (None, _) => true,
        _ => false,
    };

    let remote = match tip {
        Some(tip) => {
            let remote_ref = id.remote_ref_name(&preferences.remote);
            match project.resolve_ref(&remote_ref)? {
                Some(remote_tip) => Some((remote_ref, project.ahead_behind(tip, remote_tip)?)),
                None => None,
            }
        }
        None => None,
    };

    let rendering_count = match tip {
        Some(tip) => count_renderings(project, tip)?,
        None => 0,
    };

    Ok(Status {
        source: config.template.source,
        id,
        ref_name,
        tip,
        recorded,
        available_revision_description,
        template_moved,
        merged,
        remote,
        worktree_clean: project.is_clean()?,
        rendering_count,
    })
}

/// How many commits the rendered ref holds.
fn count_renderings(project: &dyn GitBackend, tip: Oid) -> Result<usize, GitError> {
    let mut count = 0;
    let mut current = Some(tip);
    while let Some(oid) = current {
        count += 1;
        // First parent only: the ref is a linear chain of renderings unless the
        // user merged a remote copy into it, and in that case the first-parent
        // count is still the meaningful "how many times did I render?".
        current = project.commit(oid)?.parents.first().copied();
        if count > 10_000 {
            // A guard, not a limit anyone should reach. Better than looping
            // forever on a malformed ref.
            break;
        }
    }
    Ok(count)
}

/// A `--dirty` preview: the commit, and what `.gitignore` kept out of it.
///
/// A struct rather than a bare `Oid` so that the ignored paths reach the
/// caller. They are only ever non-empty under `--dirty`, which is exactly when
/// a preview is being taken, and a preview that silently omitted files would
/// be answering a different question than the one asked.
pub struct Preview {
    /// The commit the preview was rendered into. No ref points at it.
    pub commit: Oid,
    /// Template files a `.gitignore` removed from the rendering.
    pub ignored: Vec<String>,
}

/// Render the configured template now, as a commit nothing points at.
///
/// This is what `diff --dirty` and `show --dirty` preview against. The commit
/// is a loose object: no ref is created or moved, so the append-only guarantee
/// is untouched and `git gc` reclaims it. It exists at all only because
/// [`GitBackend::merge_preview`] merges *commits*, and a rendering is a tree.
///
/// Answers come from `.config/git.tpl.toml`, so the preview asks nothing: the
/// question being answered is "what would my template edit do to this
/// project?", not "what would a different set of answers do?".
pub fn render_preview(
    project: &dyn GitBackend,
    project_root: &Path,
    overrides: BTreeMap<String, Value>,
    dirty: bool,
    user: &UserConfig,
    answering: Answering<'_>,
    trust: Trust<'_>,
) -> Result<Preview, OpError> {
    let config = Config::load(project_root)?;

    // Recorded answers first, then command-line overrides — the same order
    // `update` uses. Without the recorded ones a preview would prompt for
    // every question the project already answered, which for a
    // non-interactive caller means hanging and for an interactive one means
    // answering the questionnaire again to look at a diff.
    let mut supplied = config.answers.clone();
    supplied.extend(overrides);

    let rendered = render(
        project,
        project_root,
        &config,
        supplied,
        dirty,
        user,
        answering,
        trust,
    )?;

    // Parented on the rendered ref's tip when there is one, so the merge base
    // is the same one a real update would produce and the preview matches what
    // merging would actually do.
    let (_, ref_name) = identify(project_root)?;
    let parents: Vec<Oid> = project.resolve_ref(&ref_name)?.into_iter().collect();

    Ok(Preview {
        commit: project.create_commit(
            rendered.tree,
            &parents,
            "preview: uncommitted template\n",
        )?,
        ignored: rendered.template.ignored,
    })
}

/// A template, statically analysed.
///
/// The resolution is carried alongside the findings because every caller needs
/// both: the findings to report, and the manifest's name to head the report
/// with.
pub struct Linted {
    /// The template the findings are about.
    pub template: Resolved,
    /// What the analysis found, before any `--deny`/`--allow` policy.
    pub findings: Vec<crate::lint::Finding>,
}

/// Resolve a template and analyse it, without rendering it.
///
/// Here rather than in the command module so that `lint`'s semantics can be
/// exercised without going through the CLI, and so that nothing below `ops`
/// has to know a `lint` command exists.
///
/// Severity policy — `--deny` and `--allow` — is deliberately *not* applied
/// here. It is a decision about how to present findings, not about what the
/// template contains, and the command layer owns presentation.
pub fn lint(request: Request<'_>) -> Result<Linted, OpError> {
    let template = resolve::resolve(request)?;

    let entries = template.entries()?;
    // The whole repository, not just the render root: a `note_file` names a
    // path beside the manifest, in the same namespace a partial lives in.
    let repo_entries = template.repo.list_tree(template.tree)?;
    let partials = template.partials()?;

    // The raw manifest, because a key absorbed by a preceding table header is
    // gone once the manifest is deserialised. Read here rather than kept on
    // `Resolved`, so `init`, `update` and `render` do not carry a `String`
    // none of them reads. `resolve` has already failed if the path is absent,
    // so the fallback is unreachable.
    let manifest_bytes = template
        .repo
        .read_path(template.tree, MANIFEST_NAME)?
        .unwrap_or_default();
    let manifest_text = String::from_utf8_lossy(&manifest_bytes);

    let findings = crate::lint::lint(
        template.repo.as_ref(),
        &template.manifest,
        &manifest_text,
        &entries,
        &repo_entries,
        &partials,
    )?;

    Ok(Linted { template, findings })
}

/// A template's answer schema, in the order the questions are asked.
pub struct Questionnaire {
    /// The template the questions belong to.
    pub template: Resolved,
    /// Question names in resolution order.
    ///
    /// Names rather than borrowed `Question`s, so this does not borrow from
    /// the `Resolved` it travels with. The caller looks each one up in
    /// `template.manifest.questions`.
    pub order: Vec<String>,
}

/// Resolve a template and compute the order its questions are asked in.
///
/// Resolution order, not declaration order: when a `when` or a `default`
/// references an earlier answer, this is the order a caller has to answer in,
/// and it is the order the graph already computes for prompting.
pub fn questions(request: Request<'_>) -> Result<Questionnaire, OpError> {
    let template = resolve::resolve(request)?;
    let graph = Graph::build(&template.manifest)?;

    let order: Vec<String> = graph
        .order()
        .iter()
        .filter(|node| node.kind == crate::graph::NodeKind::Question)
        .map(|node| node.key.clone())
        // A node the manifest does not declare as a question cannot be
        // answered, so it has no place in an answer schema.
        .filter(|key| template.manifest.questions.contains_key(key))
        .collect();

    Ok(Questionnaire { template, order })
}

/// The rendered ref's tip, or a helpful error.
fn require_tip(project: &dyn GitBackend, ref_name: &str) -> Result<Oid, OpError> {
    project
        .resolve_ref(ref_name)?
        .ok_or_else(|| OpError::NoRenderedRef {
            ref_name: ref_name.to_string(),
        })
}

/// The template id and ref name for a project.
pub fn identify(project_root: &Path) -> Result<(TemplateId, String), OpError> {
    let config = Config::load(project_root)?;
    let id = TemplateId::resolve(&config.template.source, config.template.id.as_deref())?;
    let ref_name = id.ref_name();
    Ok((id, ref_name))
}

/// A diff of what merging the template would change, in whichever shape was
/// asked for, together with the paths that would conflict.
pub struct DiffPreview<T> {
    /// The changes themselves: a patch, a path list, or a diffstat.
    pub changes: T,
    /// The paths the merge could not resolve, shown with conflict markers.
    pub conflicts: Vec<String>,
}

/// The two trees a diff runs between, oriented, and the conflicts on the way.
///
/// The second endpoint is the tree a *merge* would produce, not the rendered
/// ref's tree. Diffing `HEAD` against the ref directly reports every file the
/// project owns and the template never produced as a deletion — a merge deletes
/// none of them, because they are in the merge base.
///
/// Every diff mode resolves the same pair and applies the same `--reverse`
/// swap; doing it once is what keeps `--stat` from disagreeing with the patch
/// about which direction it is reporting.
fn diff_endpoints(
    project: &dyn GitBackend,
    project_root: &Path,
    reverse: bool,
    against: Option<Oid>,
) -> Result<(Option<Oid>, Oid, Vec<String>), OpError> {
    // `against` is a commit to preview instead of the rendered ref's tip — a
    // rendering that exists only as an object, never as a ref, which is how
    // `--dirty` previews an uncommitted template without writing anything.
    let tip = match against {
        Some(commit) => commit,
        None => {
            let (_, ref_name) = identify(project_root)?;
            require_tip(project, &ref_name)?
        }
    };

    let Some(head) = project.head_commit()? else {
        // No commits yet: the merge is the fast-forward that creates them, so
        // the whole rendering is an addition. Reversed, there is nothing to
        // diff against, and the empty answer is the truth.
        let template_tree = project.commit(tip)?.tree;
        return Ok(if reverse {
            (Some(template_tree), template_tree, Vec::new())
        } else {
            (None, template_tree, Vec::new())
        });
    };

    let head_tree = project.commit(head)?.tree;
    let preview = project.merge_preview(head, tip)?;

    Ok(if reverse {
        (Some(preview.tree), head_tree, preview.conflicts)
    } else {
        (Some(head_tree), preview.tree, preview.conflicts)
    })
}

/// The patch merging the template would apply.
pub fn diff(
    project: &dyn GitBackend,
    project_root: &Path,
    paths: &[String],
    reverse: bool,
    against: Option<Oid>,
) -> Result<DiffPreview<String>, OpError> {
    let (from, to, conflicts) = diff_endpoints(project, project_root, reverse, against)?;
    Ok(DiffPreview {
        changes: project.diff_patch(from, to, paths)?,
        conflicts,
    })
}

/// The changes merging the template would make.
pub fn diff_changes(
    project: &dyn GitBackend,
    project_root: &Path,
    paths: &[String],
    reverse: bool,
    against: Option<Oid>,
) -> Result<DiffPreview<Vec<Change>>, OpError> {
    let (from, to, conflicts) = diff_endpoints(project, project_root, reverse, against)?;
    Ok(DiffPreview {
        changes: project.diff_trees(from, to, paths)?,
        conflicts,
    })
}

/// The changes merging the template would make, with their line counts.
pub fn diff_stat(
    project: &dyn GitBackend,
    project_root: &Path,
    paths: &[String],
    reverse: bool,
    against: Option<Oid>,
) -> Result<DiffPreview<Vec<FileStat>>, OpError> {
    let (from, to, conflicts) = diff_endpoints(project, project_root, reverse, against)?;
    Ok(DiffPreview {
        changes: project.diff_stat(from, to, paths)?,
        conflicts,
    })
}

/// What [`show`] found at a path.
pub enum Shown {
    /// A file, and its bytes exactly as rendered.
    File(Vec<u8>),
    /// A directory, and the root-relative paths beneath it, sorted.
    Directory(Vec<String>),
}

/// Normalise a path argument into the form a Git tree lookup expects.
///
/// Root-relative, no leading `./`, no trailing `/`. An absolute path or a `..`
/// component is refused rather than resolved: a tree lookup cannot escape a
/// tree, so this is not a security boundary, but `read_path` would answer a
/// bare "not found" for `../x` and send the reader looking in the wrong place.
fn normalise_shown_path(path: &str) -> Result<String, OpError> {
    let trimmed = path.trim_end_matches('/');
    let trimmed = trimmed.strip_prefix("./").unwrap_or(trimmed);

    if trimmed.starts_with('/') {
        return Err(OpError::InvalidArgument {
            message: format!("`{path}` is absolute; paths are relative to the repository root"),
        });
    }
    if trimmed.split('/').any(|part| part == "..") {
        return Err(OpError::InvalidArgument {
            message: format!(
                "`{path}` leaves the rendering; paths are relative to the repository root"
            ),
        });
    }

    // `.` and `./` name the whole rendering, which `subtree` already answers
    // with the root tree. Reduced to the empty string so there is one spelling
    // of "the root" below rather than three.
    Ok(if trimmed == "." {
        String::new()
    } else {
        trimmed.to_string()
    })
}

/// One path as the template renders it, read from `refs/tpl/<id>`.
///
/// The ref tip, and only the ref tip. The motivating moment is a conflicted
/// merge, where the tip is the side being read and the machine may be offline —
/// so this never resolves the template repository and never touches the
/// network.
pub fn show(
    project: &dyn GitBackend,
    project_root: &Path,
    path: &str,
    against: Option<Oid>,
) -> Result<Shown, OpError> {
    // Resolved even when `against` supplies the tree, because the
    // "no such path" diagnostic names the ref the reader was looking in, and a
    // preview is still a rendering *of* that template.
    let (_, ref_name) = identify(project_root)?;

    // As in `diff_endpoints`: `against` is a rendering that exists as an
    // object but not as a ref, so `--dirty` can show a file from an
    // uncommitted template without writing anything.
    let tree = match against {
        Some(commit) => project.commit(commit)?.tree,
        None => {
            let tip = require_tip(project, &ref_name)?;
            project.commit(tip)?.tree
        }
    };

    let path = normalise_shown_path(path)?;

    // Asked before `read_path` on purpose: the backend's `read_path` calls
    // `find_blob` on a tree oid and fails with an opaque "could not read the
    // file" when the path is a directory.
    if let Some(subtree) = project.subtree(tree, &path)? {
        let entries = project.list_tree(subtree)?;
        let paths = entries
            .into_iter()
            // `list_tree` yields paths relative to the tree it was given, so
            // the prefix goes back on: everything this command prints is
            // root-relative, like `git tpl diff --name-only`.
            .map(|entry| {
                if path.is_empty() {
                    entry.path
                } else {
                    format!("{path}/{}", entry.path)
                }
            })
            .collect();
        return Ok(Shown::Directory(paths));
    }

    match project.read_path(tree, &path)? {
        Some(bytes) => Ok(Shown::File(bytes)),
        None => Err(OpError::NoSuchPath { path, ref_name }),
    }
}

/// Merge the rendered ref into the current branch.
///
/// Delegates entirely to the backend's merge. git-tpl contributes no conflict
/// resolution — see `docs/adr/002-no-custom-reconciliation.md`.
pub fn merge(
    project: &dyn GitBackend,
    project_root: &Path,
    message: Option<&str>,
    commit_result: bool,
) -> Result<(TemplateId, MergeOutcome), OpError> {
    let (id, ref_name) = identify(project_root)?;
    let tip = require_tip(project, &ref_name)?;

    let message = message.map(str::to_string).unwrap_or_else(|| {
        format!("Merge {ref_name}\n\nTemplate changes rendered by `git tpl update`.\n")
    });

    let outcome = project.merge(tip, &message, commit_result, &[])?;
    Ok((id, outcome))
}

/// Fetch template refs from a remote.
///
/// Never moves the local ref. What to do about a newer remote copy is the
/// user's decision, and adopting someone else's rendering silently would be a
/// surprising thing for a fetch to do.
/// Returns the ref it compared, so the caller does not have to `identify` a
/// second time to name it in the report.
pub fn fetch(
    project: &dyn GitBackend,
    project_root: &Path,
    preferences: &Preferences,
) -> Result<(String, Option<AheadBehind>), OpError> {
    let (id, ref_name) = identify(project_root)?;

    project.fetch_refspec(&preferences.remote, &preferences.fetch_refspec())?;

    let remote_ref = id.remote_ref_name(&preferences.remote);
    let relation = match (
        project.resolve_ref(&ref_name)?,
        project.resolve_ref(&remote_ref)?,
    ) {
        (Some(local), Some(remote)) => Some(project.ahead_behind(local, remote)?),
        _ => None,
    };
    Ok((ref_name, relation))
}

/// Push the rendered ref to a remote.
///
/// Refuses to push a diverged ref, and offers no way to force. A rendered ref
/// is history others may have merged from; overwriting it destroys the merge
/// base their next update needs.
pub fn push(
    project: &dyn GitBackend,
    project_root: &Path,
    preferences: &Preferences,
) -> Result<String, OpError> {
    let (id, ref_name) = identify(project_root)?;
    let tip = require_tip(project, &ref_name)?;

    let remote_ref = id.remote_ref_name(&preferences.remote);
    if let Some(remote_tip) = project.resolve_ref(&remote_ref)? {
        let relation = project.ahead_behind(tip, remote_tip)?;
        if relation.is_diverged() {
            return Err(GitError::Diverged {
                ref_name: ref_name.clone(),
                remote_ref,
                ahead: relation.ahead,
                behind: relation.behind,
            }
            .into());
        }
    }

    project.push_refspec(&preferences.remote, &push_refspec(&ref_name))?;
    Ok(ref_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A manifest with one question of each of the kinds these tests need.
    fn manifest_with(questions: &str) -> Manifest {
        Manifest::parse(&format!("name = \"t\"\n{questions}"), "template.toml").unwrap()
    }

    fn user_with(defaults: &str) -> UserConfig {
        UserConfig::parse(&format!("[defaults]\n{defaults}"), "config.toml").unwrap()
    }

    #[test]
    fn a_user_default_overrides_a_git_seeded_one() {
        // `default_from` is the template author guessing where the answer comes
        // from; `[defaults]` is the person at the keyboard saying it outright.
        let manifest = manifest_with("[questions.author]\ntype = \"string\"\n");
        let mut seeds = BTreeMap::from([("author".to_string(), Value::String("From Git".into()))]);

        apply_user_defaults(
            &mut seeds,
            &manifest,
            &user_with("author = \"From The File\"\n"),
        );

        assert_eq!(
            seeds.get("author"),
            Some(&Value::String("From The File".into()))
        );
    }

    mod derived_seeds {
        use super::*;
        use crate::git::libgit2::LibGit2;

        fn project(name: &str) -> (tempfile::TempDir, LibGit2) {
            let parent = tempfile::tempdir().unwrap();
            let path = parent.path().join(name);
            std::fs::create_dir(&path).unwrap();
            let repo = LibGit2::init(&path).unwrap();
            (parent, repo)
        }

        const SLUG: &str = "[questions.slug]\ntype = \"string\"\n\
             default_from = \"{{ remote.name | default(dir.name) | slugify }}\"\n";

        #[test]
        fn a_remote_seeds_the_prompt() {
            let (_dir, repo) = project("some-checkout");
            repo.set_config_str("remote.origin.url", "git@github.com:me/Git Tpl.git")
                .unwrap();

            let seeds = prompt_seeds(&repo, &manifest_with(SLUG), &user_with("")).unwrap();

            assert_eq!(seeds.get("slug"), Some(&Value::String("git-tpl".into())));
        }

        /// The case the fallback exists for: a project created locally and not
        /// yet pushed anywhere.
        #[test]
        fn without_a_remote_the_directory_name_seeds_the_prompt() {
            let (_dir, repo) = project("My Project");

            let seeds = prompt_seeds(&repo, &manifest_with(SLUG), &user_with("")).unwrap();

            assert_eq!(seeds.get("slug"), Some(&Value::String("my-project".into())));
        }

        /// Precedence is unchanged by the new form: the person at the keyboard
        /// still outranks the template author's guess.
        #[test]
        fn a_user_default_still_beats_a_derived_seed() {
            let (_dir, repo) = project("some-checkout");
            repo.set_config_str("remote.origin.url", "git@github.com:me/guessed.git")
                .unwrap();

            let seeds = prompt_seeds(
                &repo,
                &manifest_with(SLUG),
                &user_with("slug = \"stated-outright\"\n"),
            )
            .unwrap();

            assert_eq!(
                seeds.get("slug"),
                Some(&Value::String("stated-outright".into()))
            );
        }

        /// An expression yielding nothing is an absent seed, not an empty
        /// prompt — the same rule an unset configuration key follows.
        #[test]
        fn an_expression_resolving_to_nothing_seeds_nothing() {
            let (_dir, repo) = project("whatever");

            let seeds = prompt_seeds(
                &repo,
                &manifest_with(
                    "[questions.author]\ntype = \"string\"\n\
                     default_from = \"{{ git.user.nickname }}\"\n",
                ),
                &user_with(""),
            )
            .unwrap();

            assert!(seeds.is_empty(), "expected no seed, got {seeds:?}");
        }
    }

    #[test]
    fn a_user_default_naming_no_question_is_skipped() {
        // Silently, unlike an ignored `--answers-from` key: this file is
        // written once for every template the user will ever generate, so it is
        // expected to overshoot.
        let manifest = manifest_with("[questions.author]\ntype = \"string\"\n");
        let mut seeds = BTreeMap::new();

        apply_user_defaults(&mut seeds, &manifest, &user_with("licence = \"MIT\"\n"));

        assert!(seeds.is_empty());
    }

    #[test]
    fn a_user_default_of_the_wrong_type_is_skipped() {
        // A collision with an unrelated template's question of the same name.
        // Pre-filling a boolean prompt with a string is worse than not
        // pre-filling it.
        let manifest = manifest_with("[questions.ci]\ntype = \"boolean\"\n");
        let mut seeds = BTreeMap::new();

        apply_user_defaults(&mut seeds, &manifest, &user_with("ci = \"yes please\"\n"));

        assert!(seeds.is_empty());
    }

    #[test]
    fn a_user_default_seeds_a_question_of_any_kind() {
        // Not only `string`, which is all `default_from` may seed. The whole
        // point of the file is `license = "MIT"`, and `license` is a choice.
        let manifest = manifest_with(
            "[questions.license]\ntype = \"choice\"\nchoices = [\"MIT\", \"Apache-2.0\"]\n\n\
             [questions.ci]\ntype = \"boolean\"\n",
        );
        let mut seeds = BTreeMap::new();

        apply_user_defaults(
            &mut seeds,
            &manifest,
            &user_with("license = \"MIT\"\nci = true\n"),
        );

        assert_eq!(seeds.get("license"), Some(&Value::String("MIT".into())));
        assert_eq!(seeds.get("ci"), Some(&Value::Bool(true)));
    }

    #[test]
    fn a_status_with_nothing_outstanding_is_not_pending() {
        let status = Status {
            source: "../tpl".into(),
            id: TemplateId::explicit("tpl").unwrap(),
            ref_name: "refs/tpl/tpl".into(),
            tip: Some(Oid::from_bytes([1; 20])),
            recorded: None,
            available_revision_description: None,
            template_moved: false,
            merged: true,
            remote: None,
            worktree_clean: true,
            rendering_count: 1,
        };
        assert!(!status.is_pending());
    }

    /// `git tpl status --quiet` is meant to be usable as a CI drift check, so
    /// the pending condition has to cover both ways of being out of date.
    #[test]
    fn a_moved_template_or_an_unmerged_rendering_is_pending() {
        let base = Status {
            source: "../tpl".into(),
            id: TemplateId::explicit("tpl").unwrap(),
            ref_name: "refs/tpl/tpl".into(),
            tip: Some(Oid::from_bytes([1; 20])),
            recorded: None,
            available_revision_description: None,
            template_moved: false,
            merged: true,
            remote: None,
            worktree_clean: true,
            rendering_count: 1,
        };

        assert!(
            Status {
                template_moved: true,
                ..base
            }
            .is_pending()
        );
    }
}
