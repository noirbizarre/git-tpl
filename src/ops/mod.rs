//! Orchestration — one operation per command.
//!
//! Everything below this module is unaware that commands exist. These
//! functions compose resolution, evaluation, rendering and Git into the
//! operations the CLI exposes.

pub mod resolve;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use miette::Diagnostic;
use thiserror::Error;

use crate::config::{CONFIG_PATH, Config, ConfigError};
use crate::context::Context;
use crate::data::{Loader, TemplateTree};
use crate::eval::{DefaultsOnly, EvalError, Evaluation, Prompter};
use crate::git::libgit2::LibGit2;
use crate::git::{AheadBehind, Change, GitBackend, GitError, MergeOutcome, Oid};
use crate::gitconfig::{Preferences, push_refspec};
use crate::graph::{Graph, GraphError};
use crate::provenance::{Provenance, Recorded};
use crate::refs::{TemplateId, TemplateIdError};
use crate::render::{RenderError, render_tree};
use crate::template::Value;

pub use resolve::{Request, ResolveError, Resolved};

/// Errors from any operation.
#[derive(Debug, Error, Diagnostic)]
pub enum OpError {
    /// The project configuration is missing or invalid.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Config(#[from] ConfigError),

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

    /// Rendering failed.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Render(#[from] RenderError),

    /// A Git operation failed.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Git(#[from] GitError),

    /// The template id could not be determined.
    #[error(transparent)]
    #[diagnostic(transparent)]
    TemplateId(#[from] TemplateIdError),

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
}

/// Resolve, evaluate and render — everything short of touching a ref.
///
/// Shared by `init`, `update` and `--dry-run`, so all three cannot disagree
/// about what a rendering is.
pub fn render(
    project: &LibGit2,
    project_root: &Path,
    config: &Config,
    supplied: BTreeMap<String, Value>,
    dirty: bool,
    mut answering: Answering<'_>,
) -> Result<Render, OpError> {
    let template = resolve::resolve(Request {
        source: &config.template.source,
        reference: config.template.r#ref.as_deref(),
        root: config.template.root.as_deref(),
        dirty,
    })?;

    // Built and validated before anything is prompted: a cycle or a typo
    // discovered after six answered questions is the worst possible time.
    let graph = Graph::build(&template.manifest)?;

    let mut loader = Loader::new(
        TemplateTree {
            repo: &template.repo,
            tree: template.tree,
            revision: template.revision,
        },
        project_root,
    );

    let context = crate::eval::resolve(
        Evaluation {
            manifest: &template.manifest,
            graph: &graph,
            supplied,
        },
        &mut loader,
        answering.prompter(),
    )?;

    let entries = template.entries()?;
    // Blobs are read from the template repository — often a temporary clone —
    // and written into the project, which is where the ref will point.
    let tree = render_tree(&template.repo, project, &entries, &context)?;

    let provenance = Provenance {
        source: config.template.source.clone(),
        reference: template.reference.clone(),
        commit: template.revision,
        dirty: template.dirty,
        answers_digest: context.answers_digest(),
        data: loader.provenance().to_vec(),
        version: crate::VERSION.to_string(),
        template_name: template.manifest.name.clone(),
    };

    Ok(Render {
        template,
        context,
        tree,
        provenance,
    })
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
    /// The revision that was rendered.
    pub revision: String,
}

/// Attach a template to a repository and merge the initial rendering.
///
/// The merge is the load-bearing step: without it the template commit is not an
/// ancestor of the branch, so the *first* `update` would have no merge base and
/// would conflict on every line of every file. See
/// `docs/adr/009-init-merges-unrelated-histories.md`.
#[allow(clippy::too_many_arguments)]
pub fn init(
    project: &LibGit2,
    project_root: &Path,
    source: &str,
    reference: Option<String>,
    explicit_id: Option<&str>,
    supplied: BTreeMap<String, Value>,
    dirty: bool,
    merge_after: bool,
    answering: Answering<'_>,
) -> Result<InitOutcome, OpError> {
    if Config::exists_in(project_root) {
        return Err(OpError::AlreadyInitialised);
    }

    // A merge needs a clean worktree, and so does this. Checked before doing
    // any work rather than after the questionnaire.
    if merge_after && !project.is_empty()? && !project.is_clean()? {
        return Err(GitError::DirtyWorktree.into());
    }

    let mut config = Config::new(source, reference);
    config.template.id = explicit_id.map(str::to_string);

    let rendered = render(project, project_root, &config, supplied, dirty, answering)?;

    let id = TemplateId::resolve(source, explicit_id)?;
    let ref_name = id.ref_name();

    // An orphan commit: the template has no history in this project before
    // now, and inventing a parent would be a lie.
    let commit = project.create_commit(rendered.tree, &[], &rendered.provenance.to_message())?;
    project.set_ref(&ref_name, commit, "tpl: initial render")?;

    let changes = project.diff_trees(None, rendered.tree)?;

    // Written after rendering, so a template that fails to render leaves no
    // half-initialised project behind.
    config.answers = rendered.context.answers().clone();
    let config_path = config.save(project_root)?;

    let merge = if merge_after {
        let outcome = project.merge(
            commit,
            &format!(
                "Merge template {} into {}\n\n\
                 Initial rendering of the template attached by `git tpl init`.\n",
                rendered.template.manifest.name,
                project
                    .head_branch()?
                    .unwrap_or_else(|| "the branch".into())
            ),
            true,
        )?;
        Some(outcome)
    } else {
        None
    };

    // `.config/git.tpl.toml` is versioned with the project — a fresh clone must
    // be understandable from it alone. Leaving it untracked would mean the
    // template attachment existed only on the machine that ran `init`.
    //
    // Staged after the merge, not before: a dirty index makes libgit2 refuse to
    // merge, and the failure would be about the index rather than about
    // anything the user did.
    project.stage(Path::new(CONFIG_PATH))?;

    let config_committed = match &merge {
        // Conflicts are the user's to resolve, and their resolution commit is
        // where the configuration belongs. Committing now would make a commit
        // in the middle of a merge they have not finished.
        Some(MergeOutcome::Conflicted { .. }) => false,
        _ => {
            project.commit_index(&format!(
                "chore(tpl): attach the {} template\n\n\
                 Records the template source and the answers used to render it.\n\
                 See {CONFIG_PATH}.\n",
                rendered.template.manifest.name
            ))?;
            true
        }
    };

    Ok(InitOutcome {
        id,
        commit,
        changes,
        merge,
        config_path,
        config_committed,
        revision: describe_revision(&rendered.template.reference, rendered.template.revision),
    })
}

/// The result of an `update`.
pub enum UpdateOutcome {
    /// The rendered tree was identical to the ref's tip; nothing was committed.
    ///
    /// The reason determinism matters: a renderer that varied would create a
    /// commit on every run, and every one would be noise to merge.
    UpToDate {
        /// The revision that was rendered.
        revision: String,
    },
    /// A new commit was added to the rendered ref.
    Updated {
        /// The template id.
        id: TemplateId,
        /// The new commit.
        commit: Oid,
        /// What changed against the previous rendering.
        changes: Vec<Change>,
        /// The revision previously rendered, if there was one.
        previous_revision: Option<String>,
        /// The revision now rendered.
        revision: String,
        /// Whether the recorded answers were rewritten — which happens when a
        /// template adds a question. Worth telling the user, since it is the
        /// one file `update` does modify.
        answers_changed: bool,
    },
}

/// Re-render and advance the rendered ref.
///
/// Never touches `HEAD`, the index or the worktree. That is structural: the
/// tree is built as a Git object and one ref is moved. There is no code path
/// here that writes a file into the project.
pub fn update(
    project: &LibGit2,
    project_root: &Path,
    overrides: BTreeMap<String, Value>,
    dirty: bool,
    answering: Answering<'_>,
) -> Result<UpdateOutcome, OpError> {
    let mut config = Config::load(project_root)?;

    // Recorded answers first, then command-line overrides. A question added to
    // the template since the last render has no recorded answer and is
    // prompted for.
    let mut supplied = config.answers.clone();
    supplied.extend(overrides);

    let rendered = render(project, project_root, &config, supplied, dirty, answering)?;

    let id = TemplateId::resolve(&config.template.source, config.template.id.as_deref())?;
    let ref_name = id.ref_name();
    let tip = project.resolve_ref(&ref_name)?;

    let previous = tip.map(|oid| project.commit(oid)).transpose()?;
    let previous_revision = previous
        .as_ref()
        .and_then(|commit| Provenance::parse(&commit.message))
        .map(|recorded| recorded.describe_revision());

    // Identical output. Committing would add a commit that changes nothing,
    // which the user would then have to merge for no reason. This is what the
    // determinism guarantee buys.
    if let Some(previous) = &previous
        && previous.tree == rendered.tree
    {
        return Ok(UpdateOutcome::UpToDate {
            revision: describe_revision(&rendered.template.reference, rendered.template.revision),
        });
    }

    // Append-only. The parent is the current tip, whatever the reason for
    // re-rendering — template moved, answer changed, data changed. Rewriting
    // would destroy the merge base the branch already shares with the ref.
    // See docs/adr/005-append-only-refs.md.
    let parents: Vec<Oid> = tip.into_iter().collect();
    let commit =
        project.create_commit(rendered.tree, &parents, &rendered.provenance.to_message())?;
    project.set_ref(&ref_name, commit, "tpl: update")?;

    let changes = project.diff_trees(previous.as_ref().map(|c| c.tree), rendered.tree)?;

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
        previous_revision,
        revision: describe_revision(&rendered.template.reference, rendered.template.revision),
        answers_changed,
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
    pub available_revision: Option<String>,
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
    project: &LibGit2,
    project_root: &Path,
    preferences: &Preferences,
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
    let resolved = resolve::resolve(Request {
        source: &config.template.source,
        reference: config.template.r#ref.as_deref(),
        root: config.template.root.as_deref(),
        dirty: false,
    })
    .ok();

    let available_revision = resolved
        .as_ref()
        .map(|r| format!("{} ({})", r.reference, r.revision.short()));

    let template_moved = match (&resolved, &recorded) {
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
        available_revision,
        template_moved,
        merged,
        remote,
        worktree_clean: project.is_clean()?,
        rendering_count,
    })
}

/// How many commits the rendered ref holds.
fn count_renderings(project: &LibGit2, tip: Oid) -> Result<usize, GitError> {
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

/// The rendered ref's tip, or a helpful error.
fn require_tip(project: &LibGit2, ref_name: &str) -> Result<Oid, OpError> {
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

/// The difference between `HEAD` and the rendered ref.
pub fn diff(
    project: &LibGit2,
    project_root: &Path,
    paths: &[String],
    reverse: bool,
) -> Result<String, OpError> {
    let (_, ref_name) = identify(project_root)?;
    let tip = require_tip(project, &ref_name)?;

    let head_tree = match project.head_commit()? {
        Some(oid) => Some(project.commit(oid)?.tree),
        None => None,
    };
    let template_tree = project.commit(tip)?.tree;

    let (from, to) = if reverse {
        (Some(template_tree), head_tree.unwrap_or(template_tree))
    } else {
        (head_tree, template_tree)
    };

    Ok(project.diff_patch(from, to, paths)?)
}

/// The changes between `HEAD` and the rendered ref.
pub fn diff_changes(project: &LibGit2, project_root: &Path) -> Result<Vec<Change>, OpError> {
    let (_, ref_name) = identify(project_root)?;
    let tip = require_tip(project, &ref_name)?;

    let head_tree = match project.head_commit()? {
        Some(oid) => Some(project.commit(oid)?.tree),
        None => None,
    };

    Ok(project.diff_trees(head_tree, project.commit(tip)?.tree)?)
}

/// Merge the rendered ref into the current branch.
///
/// Delegates entirely to the backend's merge. git-tpl contributes no conflict
/// resolution — see `docs/adr/002-no-custom-reconciliation.md`.
pub fn merge(
    project: &LibGit2,
    project_root: &Path,
    message: Option<&str>,
    commit_result: bool,
) -> Result<(TemplateId, MergeOutcome), OpError> {
    let (id, ref_name) = identify(project_root)?;
    let tip = require_tip(project, &ref_name)?;

    let message = message.map(str::to_string).unwrap_or_else(|| {
        format!("Merge {ref_name}\n\nTemplate changes rendered by `git tpl update`.\n")
    });

    let outcome = project.merge(tip, &message, commit_result)?;
    Ok((id, outcome))
}

/// Fetch template refs from a remote.
///
/// Never moves the local ref. What to do about a newer remote copy is the
/// user's decision, and adopting someone else's rendering silently would be a
/// surprising thing for a fetch to do.
pub fn fetch(
    project: &LibGit2,
    project_root: &Path,
    preferences: &Preferences,
) -> Result<Option<AheadBehind>, OpError> {
    let (id, ref_name) = identify(project_root)?;

    project.fetch_refspec(&preferences.remote, &preferences.fetch_refspec())?;

    let remote_ref = id.remote_ref_name(&preferences.remote);
    match (
        project.resolve_ref(&ref_name)?,
        project.resolve_ref(&remote_ref)?,
    ) {
        (Some(local), Some(remote)) => Ok(Some(project.ahead_behind(local, remote)?)),
        _ => Ok(None),
    }
}

/// Push the rendered ref to a remote.
///
/// Refuses to push a diverged ref, and offers no way to force. A rendered ref
/// is history others may have merged from; overwriting it destroys the merge
/// base their next update needs.
pub fn push(
    project: &LibGit2,
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

    #[test]
    fn a_status_with_nothing_outstanding_is_not_pending() {
        let status = Status {
            source: "../tpl".into(),
            id: TemplateId::explicit("tpl").unwrap(),
            ref_name: "refs/tpl/tpl".into(),
            tip: Some(Oid::from_bytes([1; 20])),
            recorded: None,
            available_revision: None,
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
            available_revision: None,
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
