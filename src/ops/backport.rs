//! `backport` — the patch that carries a local fix back to the template.
//!
//! The whole design is ADR-020, and it rests on one sentence: rendering is
//! deterministic, so a candidate template source can be *verified* by
//! rendering it rather than inferred by pattern-matching. Read the ADR before
//! changing anything here — every refusal below buys out a way of shipping a
//! plausible-looking wrong patch to every downstream project at once.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;

use miette::Diagnostic;
use similar::{ChangeTag, DiffOp, TextDiff};
use thiserror::Error;

use crate::config::{CONFIG_PATH, Config};
use crate::context::Context;
use crate::eval::{Partials, Undefined, render_string_with};
use crate::git::{ChangeKind, GitBackend, Oid};
use crate::provenance::Provenance;
use crate::render::Rendered;
use crate::userconfig::UserConfig;

use super::hunks::{self, Hunk, Picking, Selection};
use super::unsubstitute::{
    self, LineContext, Proposal, Unsubstitute, Unsubstitution, Verdict, split_terminator,
};
use super::{Answering, OpError, Target, Trust, describe_revision, identify, render_files};
use std::sync::Arc;

/// Why a backport could not be produced.
///
/// Every variant is a refusal rather than a wrong answer, and every `help`
/// names editing the template by hand — the status quo, and therefore the
/// floor no refusal can fall below. See ADR-020.
#[derive(Debug, Error, Diagnostic)]
pub enum BackportError {
    /// A change lands on a line that rendering substituted into.
    ///
    /// The expected refusal, and the one users will meet most. Reversing the
    /// substitution is not attempted, because at the level of bytes a
    /// substitution and a coincidence are indistinguishable — see ADR-020.
    #[error("`{path}` was changed where the template substitutes a value")]
    #[diagnostic(
        code(tpl::backport::substituted_region),
        help(
            "line {line} of `{path}` is produced by an expression in `{template_path}`, not copied \
             from it, so there is no one-to-one change to send upstream. Edit `{template_path}` by \
             hand, or restrict the backport with a pathspec."
        )
    )]
    SubstitutedRegion {
        /// The rendered path, as the user sees it.
        path: String,
        /// The template source that produced it.
        template_path: String,
        /// The first offending line, 1-based, in the rendered file.
        line: usize,
    },

    /// A changed file is binary on one side or the other.
    #[error("`{path}` is binary, and cannot be backported as a patch")]
    #[diagnostic(
        code(tpl::backport::binary),
        help(
            "a text patch cannot carry it. Copy the file into `{template_path}` in the template by \
             hand, or exclude it with `--exclude {path}`."
        )
    )]
    Binary {
        /// The rendered path.
        path: String,
        /// The template source that produced it.
        template_path: String,
    },

    /// The patched source did not render back to what the project has.
    ///
    /// The proof failing. It means the change could not be placed in the
    /// source without altering something else — most often because it landed
    /// against a region a conditional collapsed.
    #[error("the backported change to `{template_path}` does not render back to `{path}`")]
    #[diagnostic(
        code(tpl::backport::round_trip),
        help(
            "the patch was built and then re-rendered to check it, and the result differed \
             from your file. Sending it would change what `{template_path}` produces for everyone. \
             Edit `{template_path}` by hand."
        )
    )]
    RoundTrip {
        /// The rendered path.
        path: String,
        /// The template source that would have been patched.
        template_path: String,
    },

    /// Re-rendering the recorded revision did not reproduce the recorded tree.
    #[error("the recorded answers no longer reproduce `{ref_name}`")]
    #[diagnostic(
        code(tpl::backport::stale_rendering),
        help(
            "`{CONFIG_PATH}` has been edited since the last render, so the ref is not what the \
             answers produce and every line of a backport would be measured against the \
             wrong file. Run `git tpl update` first."
        ),
        url("https://noirbizarre.github.io/git-tpl/usage/backport/")
    )]
    StaleRendering {
        /// The ref that disagreed.
        ref_name: String,
    },

    /// A path was named that the template does not own and does not exist.
    #[error("`{path}` is neither produced by the template nor present in the project")]
    #[diagnostic(
        code(tpl::backport::unknown_path),
        help("check the spelling. `git tpl status` lists what the template owns.")
    )]
    UnknownPath {
        /// The path as the user wrote it.
        path: String,
    },

    /// The patch could not be written to `--output`.
    #[error("could not write the patch to `{path}`")]
    #[diagnostic(code(tpl::backport::output_write), help("{reason}"))]
    OutputWrite {
        /// The path that could not be written.
        path: String,
        /// The underlying failure.
        reason: String,
    },

    /// A refusal, attributed to the hunk the user chose.
    ///
    /// A wrapper, never a cause: the refusal underneath keeps its own code and
    /// its own advice. What this adds is the one thing `-p` makes available and
    /// nothing else can — *which* of the hunks just selected was the problem,
    /// so the answer can be "run it again and leave that one out" rather than
    /// "find the line yourself".
    #[error("hunk {ordinal} of `{path}` cannot be backported")]
    #[diagnostic(
        code(tpl::backport::hunk_refused),
        help(
            "the hunk at `{hunk}` is the one that failed. Run `git tpl backport -p` again and \
             leave it out to send the rest, or edit `{template_path}` by hand to carry it."
        )
    )]
    HunkRefused {
        /// The rendered path.
        path: String,
        /// The template source that would have been patched.
        template_path: String,
        /// The hunk's position in the file, 1-based, as the picker showed it.
        ordinal: usize,
        /// The hunk's `@@` header.
        hunk: String,
        /// The refusal itself. A boxed `dyn Diagnostic` rather than a boxed
        /// `BackportError` because that is what `#[diagnostic_source]` renders
        /// in full — code, message and help — and the nested help is the whole
        /// reason to wrap rather than replace. Nothing consumes the type.
        #[diagnostic_source]
        source: Box<dyn Diagnostic + Send + Sync>,
    },

    /// The user cancelled the hunk picker.
    ///
    /// An abort, not a decline. See [`crate::ops::Picker`] for why Escape here
    /// cannot be read as "send nothing" the way it can at a confirmation.
    #[error("cancelled")]
    #[diagnostic(
        code(tpl::backport::cancelled),
        help("no patch was produced, and nothing was written. Run it again to start over.")
    )]
    Cancelled,

    /// `-p` was asked for where no prompt can run.
    #[error("`--patch` needs a terminal to ask on")]
    #[diagnostic(
        code(tpl::backport::not_interactive),
        help(
            "`--json`, a pipe, and `tpl.interactive false` all mean there is nobody to show the \
             hunks to. Select without a prompt instead: name pathspecs to limit what is \
             considered, or leave paths out with `--exclude`."
        )
    )]
    NotInteractive,
}

/// One file's worth of backported change.
#[derive(Debug, Clone)]
pub struct BackportedFile {
    /// The path in the project, as the user knows it.
    pub rendered: String,
    /// The path in the template repository, `.jinja` intact, `root` prefixed.
    pub source: String,
    /// Lines added to the template source.
    pub insertions: usize,
    /// Lines removed from the template source.
    pub deletions: usize,
    /// Whether the template source is new.
    pub added: bool,
}

/// A path that was considered and left out, with the reason.
#[derive(Debug, Clone)]
pub struct Skipped {
    /// The rendered path.
    pub path: String,
    /// A one-line reason, already user-facing.
    pub reason: String,
}

/// The outcome of a backport.
#[derive(Debug)]
pub struct Backport {
    /// The mailbox, ready for `git am`. Empty when `files` is empty.
    pub patch: String,
    /// What the patch carries.
    pub files: Vec<BackportedFile>,
    /// What was considered and deliberately left out.
    pub skipped: Vec<Skipped>,
    /// The substitutions the patch reversed, in the order they were confirmed.
    ///
    /// Reported rather than merely counted: a patch that reversed a
    /// substitution changes what the template produces for *everyone*, and it
    /// is not one to skim past. See ADR-022.
    pub unsubstituted: Vec<Unsubstitution>,
    /// The template revision the patch applies to, as `<ref> (<short>)`.
    pub revision_description: String,
    /// The template source, as configured.
    pub source: String,
    /// The command that would apply this patch, ready to paste.
    ///
    /// git-tpl does not run it — ADR-002 and ADR-020 — but it knows enough to
    /// spell it, and a user who has to reconstruct it from prose will get the
    /// `-C` wrong the first time.
    pub apply_command: String,
}

/// Produce the patch that carries the project's local divergence upstream.
///
/// Writes nothing, anywhere: not the project, not the template, not even a
/// loose object. The two trees compared both already exist in the project
/// repository, and the patch is formatted in process.
///
/// There is no `supplied` parameter, unlike every other rendering entry point.
/// The rendering here is not a rendering the user is choosing — it exists to
/// reproduce the tree the project was given, so the recorded answers are the
/// only admissible ones, and the check against the ref below would reject any
/// others anyway.
// Nine, and each one is a distinct decision the caller has already taken.
// Bundling them into a struct would move the argument list up a line without
// removing a decision from it, and hide which of them a call site forgot.
#[allow(clippy::too_many_arguments)]
pub fn backport(
    project: &dyn GitBackend,
    project_root: &Path,
    paths: &[String],
    exclude: &[String],
    user: &UserConfig,
    answering: Answering<'_>,
    trust: Trust<'_>,
    mut unsubstitute: Unsubstitute<'_>,
    mut picking: Picking<'_>,
) -> Result<Backport, OpError> {
    let config = Config::load(project_root)?;
    let (_, ref_name) = identify(project_root)?;
    let tip = super::require_tip(project, &ref_name)?;
    let recorded = Provenance::parse(&project.commit(tip)?.message);

    // The recorded revision, not the configured one. A backport diffs the
    // user's divergence, and rendering anything else folds the template's own
    // movement into the patch — which would send upstream a revert of upstream.
    let reference = recorded
        .as_ref()
        .and_then(|r| r.reference.clone())
        .or_else(|| config.template.r#ref.clone());
    let revision = recorded.as_ref().and_then(|r| r.commit);

    let rendered = render_files(
        Target {
            source: &config.template.source,
            // A SHA if we have one: a branch name would follow the branch, and
            // the whole point is to reproduce what the user actually has.
            reference: revision
                .map(|oid| oid.to_string())
                .as_deref()
                .or(reference.as_deref()),
            root: config.template.root.as_deref(),
            dirty: false,
        },
        Some((project, project_root)),
        config.answers.clone(),
        user,
        answering,
        trust,
    )?;

    // The proof depends on the rendering we hold matching the one the ref
    // records. If it does not, the alignment below is against the wrong file
    // and every line number in the patch is wrong. Checked by writing the
    // rendered bytes into the *project* — never the template — as `render`
    // already does.
    let rendered_tree = crate::render::write_tree(project, &rendered.files)?;
    if rendered_tree != project.commit(tip)?.tree {
        return Err(BackportError::StaleRendering {
            ref_name: ref_name.clone(),
        }
        .into());
    }

    let (workdir_tree, _ignored) = project.tree_from_workdir(project_root)?;

    // libgit2 does the pathspec matching, because both trees are here and
    // reimplementing Git's pathspec rules is the kind of thing that is subtly
    // wrong for years.
    let changes = project.diff_trees(Some(rendered_tree), workdir_tree, paths)?;

    let by_output: BTreeMap<&str, &Rendered> = rendered
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();

    let partials = rendered.template.partials()?;
    let undefined = if rendered.template.manifest.strict.unwrap_or(false) {
        Undefined::Strict
    } else {
        Undefined::Lenient
    };
    let root = rendered.template.root.clone();

    // Gathered once. `None` is ADR-020's behaviour exactly: a change to a line
    // the render produced is refused, and no reversal is attempted.
    let lines = match unsubstitute {
        Unsubstitute::Never => None,
        Unsubstitute::Ask(_) | Unsubstitute::Always => Some(LineContext {
            context: &rendered.context,
            partials: &partials,
            undefined,
        }),
    };

    let mut files = Vec::new();
    let mut skipped = Vec::new();
    let mut diffs = Vec::new();
    let mut unsubstituted: Vec<Unsubstitution> = Vec::new();

    for change in changes {
        if excluded(&change.path, exclude) {
            continue;
        }

        match change.kind {
            ChangeKind::Deleted => {
                // Deleting a file from a template deletes it from every project
                // that renders it. Far too blunt to infer from one project's
                // working tree, where a file may be absent for a dozen local
                // reasons. Named explicitly, it is still only reported.
                skipped.push(Skipped {
                    path: change.path.clone(),
                    reason: "deleted locally; removing it from the template would remove it \
                             from every project"
                        .to_string(),
                });
                continue;
            }
            ChangeKind::Added => {
                // Not template-owned. Only carried when the user named it, and
                // `paths` being empty means they named nothing.
                //
                // Silently, unlike a deletion: every project has files the
                // template never produced — its own source, its notes, its
                // `.config/git.tpl.toml` — and listing them all as "skipped"
                // would bury the one line that matters under the whole
                // repository. `skipped` means "you might have expected this
                // and did not get it", which a file the template has never
                // seen is not.
                if paths.is_empty() {
                    continue;
                }
                let project_bytes = read_required(project, workdir_tree, &change.path)?;
                if is_binary(&project_bytes) {
                    return Err(BackportError::Binary {
                        path: change.path.clone(),
                        template_path: change.path.clone(),
                    }
                    .into());
                }
                let text = to_text(&project_bytes, &change.path, &change.path)?;
                // An added file is one hunk — `hunks` against an empty
                // rendering says so — which makes deselecting it the way to
                // drop a file named on the command line, with no second code
                // path for the case.
                let (text, _, _) = choose(&mut picking, "", &text, &change.path, &change.path)?;
                if text.is_empty() {
                    continue;
                }
                // No `.jinja`: nothing was substituted into a file the template
                // has never seen, and naming it `.jinja` would render it —
                // turning any `{{` the user wrote into a template expression.
                let source = join_root(&root, &change.path);
                diffs.push(file_diff(&source, "", &text, true));
                files.push(BackportedFile {
                    rendered: change.path.clone(),
                    source,
                    insertions: line_count(&text),
                    deletions: 0,
                    added: true,
                });
            }
            ChangeKind::Modified => {
                let Some(file) = by_output.get(change.path.as_str()) else {
                    return Err(BackportError::UnknownPath {
                        path: change.path.clone(),
                    }
                    .into());
                };

                let template_bytes = rendered
                    .template
                    .repo
                    .read_path(rendered.template.root_tree, &file.source)?;
                let Some(template_bytes) = template_bytes else {
                    return Err(BackportError::UnknownPath {
                        path: file.source.clone(),
                    }
                    .into());
                };
                let project_bytes = read_required(project, workdir_tree, &change.path)?;

                // A change with no change to the content: `diff_trees` reports
                // a differing *mode* too, and on Windows it always does — the
                // filesystem cannot represent the executable bit, so every
                // executable file the template ships looks modified there.
                //
                // Backport carries content. There is nothing here to carry, so
                // this is silence rather than a `skipped` entry: the file was
                // not considered and dropped, it simply has no change. Leaving
                // it in emitted a file section with no hunks, which is a
                // malformed patch that `git am` rejects outright — taking the
                // rest of the patch down with it.
                if project_bytes == file.content {
                    continue;
                }

                if is_binary(&template_bytes)
                    || is_binary(&file.content)
                    || is_binary(&project_bytes)
                {
                    return Err(BackportError::Binary {
                        path: change.path.clone(),
                        template_path: file.source.clone(),
                    }
                    .into());
                }

                let source_text = to_text(&template_bytes, &change.path, &file.source)?;
                let rendered_text = to_text(&file.content, &change.path, &file.source)?;
                let project_text = to_text(&project_bytes, &change.path, &file.source)?;

                // Selection first, and only then the proof. A change that
                // round-tripped whole does not necessarily round-trip with half
                // its hunks dropped, and what ADR-020 guarantees is the patch
                // that is emitted — not one it was cut from. So everything
                // below runs on the selection, and `project_text` from here on
                // means "your file, with only the hunks you chose".
                let (project_text, hunks, chosen) = choose(
                    &mut picking,
                    &rendered_text,
                    &project_text,
                    &change.path,
                    &file.source,
                )?;

                // Nothing chosen is not a refusal; it is the user saying "not
                // this file". Silence for the same reason a mode-only change is
                // silent: there is no change left to carry, and an empty file
                // section is a patch `git am` rejects outright.
                if project_text == rendered_text {
                    continue;
                }

                let attributed = |error| attribute(error, &hunks, &chosen, &change.path, file);

                let patched = transpose(
                    &source_text,
                    &rendered_text,
                    &project_text,
                    &change.path,
                    &file.source,
                    lines.as_ref(),
                )
                .map_err(attributed)?;

                // The proof. Determinism (invariant 2) is what makes a passing
                // re-render mean something: the patched source demonstrably
                // produces the user's file, rather than looking as though it
                // might.
                if let Err(error) = verify(
                    &patched.patched,
                    &project_text,
                    file,
                    &rendered.context,
                    &partials,
                    undefined,
                    &change.path,
                ) {
                    // A failed round trip on a file that reversed a
                    // substitution is almost certainly the reversal's fault,
                    // and `round_trip`'s help tells the user the wrong story
                    // about it. Ask what the answer would have been without
                    // un-substitution and report that instead — normally
                    // `substituted_region`, which is honest and actionable.
                    if !patched.unsubstituted.is_empty() {
                        transpose(
                            &source_text,
                            &rendered_text,
                            &project_text,
                            &change.path,
                            &file.source,
                            None,
                        )
                        .map_err(attributed)?;
                    }
                    return Err(attributed(error).into());
                }

                // Only now is the user asked. Confirming a line of a patch that
                // was about to be refused anyway wastes the one decision only
                // they can make.
                confirm(
                    &mut unsubstitute,
                    &patched.unsubstituted,
                    &change.path,
                    &file.source,
                )?;

                let (insertions, deletions) = counts(&source_text, &patched.patched);
                let source = join_root(&root, &file.source);
                diffs.push(file_diff(&source, &source_text, &patched.patched, false));
                unsubstituted.extend(patched.unsubstituted.into_iter().map(|mut reversal| {
                    reversal.path = change.path.clone();
                    reversal.template_path = file.source.clone();
                    reversal
                }));
                files.push(BackportedFile {
                    rendered: change.path.clone(),
                    source,
                    insertions,
                    deletions,
                    added: false,
                });
            }
        }
    }

    // Every arm goes through `describe_revision`, arm for arm as
    // `Provenance::Recorded::describe_revision` does. Cloning the reference
    // instead — which this did — printed the literal `<worktree>` where the
    // rest of git-tpl prints `7fa834c (+ uncommitted changes)`, and the string
    // is not merely displayed: it is embedded in the emitted patch and
    // re-exported as the JSON `revision` field.
    let revision_description = match (&reference, revision) {
        (Some(reference), Some(revision)) => describe_revision(reference, revision),
        // The recorded pair is incomplete, so fall back to what the render
        // actually resolved rather than to the bare name.
        (Some(reference), None) => describe_revision(reference, rendered.template.revision),
        (None, Some(revision)) => revision.short(),
        (None, None) => describe_revision(&rendered.template.reference, rendered.template.revision),
    };

    let apply_command = apply_command(&config.template.source, project_root);

    let patch = if files.is_empty() {
        String::new()
    } else {
        mailbox(project, project_root, &revision_description, &files, &diffs)?
    };

    Ok(Backport {
        patch,
        files,
        skipped,
        unsubstituted,
        revision_description,
        source: config.template.source.clone(),
        apply_command,
    })
}

/// Cut a change into hunks, ask which to keep, and reassemble the answer.
///
/// Returns the project text as the user selected it, plus the hunks and the
/// selection — the latter two only so that a refusal further down can name the
/// hunk that caused it rather than a line number the user has to go and find.
///
/// Without `-p` this is the identity, and deliberately so: `Picking::All` is
/// not a code path, it is the absence of one.
fn choose(
    picking: &mut Picking<'_>,
    rendered: &str,
    project: &str,
    path: &str,
    template_path: &str,
) -> Result<(String, Vec<Hunk>, Vec<usize>), BackportError> {
    let Picking::Ask(picker) = picking else {
        return Ok((project.to_string(), Vec::new(), Vec::new()));
    };

    let hunks = hunks::hunks(rendered, project);
    let chosen = picker
        .pick(&Selection {
            path,
            template_path,
            hunks: &hunks,
        })
        // An abort, not an empty selection. Reading a cancelled prompt as
        // "keep nothing" would quietly emit a patch the user was in the
        // middle of assembling.
        .ok_or(BackportError::Cancelled)?;

    Ok((hunks::apply(rendered, project, &chosen), hunks, chosen))
}

/// Name the hunk a refusal came from, where one can be named.
///
/// Only `SubstitutedRegion` carries a line, and only a line inside a chosen
/// hunk can be attributed. Everything else is returned untouched: a wrapper
/// that said "one of the hunks you picked, we are not sure which" would cost
/// the user the code they were about to act on and give nothing back.
fn attribute(
    error: BackportError,
    hunks: &[Hunk],
    chosen: &[usize],
    path: &str,
    file: &Rendered,
) -> BackportError {
    let BackportError::SubstitutedRegion { line, .. } = &error else {
        return error;
    };
    let Some(hunk) = hunks::containing(hunks, chosen, line.saturating_sub(1)) else {
        return error;
    };

    BackportError::HunkRefused {
        path: path.to_string(),
        template_path: file.source.clone(),
        ordinal: hunk.index + 1,
        hunk: hunk.header.clone(),
        source: Box::new(error),
    }
}

/// The outcome of mapping a change onto the template source.
#[derive(Debug)]
struct Transposed {
    /// The patched template source.
    patched: String,
    /// The substitutions that were reversed to get there, in line order.
    ///
    /// Kept rather than confirmed on the spot: `backport` proves the whole file
    /// round-trips before it asks anything, so the user is never asked to bless
    /// a patch that was going to be refused anyway.
    unsubstituted: Vec<Unsubstitution>,
}

/// Map a change to the rendered file onto the template source that produced it.
///
/// Returns the patched source, or refuses. The alignment is what makes this
/// possible at all: a run of lines that survived rendering byte-for-byte is a
/// region where the source and the output are the same text, so a change to
/// the output is unambiguously a change to the source.
///
/// `lines` being `Some` additionally allows a line the render *did* change to
/// be reversed, when its provenance can be established byte by byte — see
/// `unsubstitute` and ADR-022. `None` is ADR-020's behaviour exactly.
fn transpose(
    source: &str,
    rendered: &str,
    project: &str,
    path: &str,
    source_path: &str,
    lines: Option<&LineContext<'_>>,
) -> Result<Transposed, BackportError> {
    // Which rendered lines came from which source lines. Only `Equal` runs
    // carry a mapping; everything else is a region rendering changed, and a
    // change landing there has no one-to-one image in the source.
    let mut map: BTreeMap<usize, usize> = BTreeMap::new();
    // For a rendered line that has no mapping, the source lines it could have
    // come from — the other half of the alignment op it sits in. Candidates
    // only: which one it really was is settled by re-rendering, not by position.
    let mut origins: BTreeMap<usize, Range<usize>> = BTreeMap::new();
    for op in TextDiff::from_lines(source, rendered).ops() {
        match *op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => {
                for offset in 0..len {
                    map.insert(new_index + offset, old_index + offset);
                }
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                for offset in 0..new_len {
                    origins.insert(new_index + offset, old_index..old_index + old_len);
                }
            }
            // Rendered lines with no source lines opposite them at all. A
            // `{% for %}` body can do this; there is nothing to try.
            DiffOp::Insert { .. } | DiffOp::Delete { .. } => {}
        }
    }

    let source_lines: Vec<&str> = split_lines(source);
    let rendered_lines: Vec<&str> = split_lines(rendered);
    let project_lines: Vec<&str> = split_lines(project);

    // Rebuild the source by walking the rendered→project diff and echoing each
    // change onto the source line it maps to. Equal lines are emitted from the
    // *source*, not the rendering, so untouched substitutions keep their
    // `{{ }}` — that is the whole trick.
    let mut out: Vec<String> = Vec::new();
    let mut emitted = 0usize; // next unemitted source line
    // Source lines un-substitution rewrote, applied wherever `emit_through`
    // passes them. Routing every emission through one helper is what makes an
    // override impossible to honour at some sites and forget at the rest.
    let mut overrides: BTreeMap<usize, String> = BTreeMap::new();
    let mut unsubstituted: Vec<Unsubstitution> = Vec::new();

    let diff = TextDiff::from_slices(&rendered_lines, &project_lines);
    for op in diff.ops() {
        // `ops` rather than `iter_all_changes`: a modified line is reported by
        // the latter as an unrelated delete and an unrelated insert, so the
        // walk below could not see that the two belong together even where it
        // matters. `Replace` keeps them in one place.
        let (deletes, inserts) = match *op {
            DiffOp::Equal {
                old_index, len: n, ..
            } => {
                for offset in 0..n {
                    if let Some(&mapped) = map.get(&(old_index + offset)) {
                        // Emit everything the rendering dropped between the
                        // last mapped line and this one — the substituted
                        // regions in between, verbatim from the source.
                        emit_through(
                            &mut out,
                            &source_lines,
                            &overrides,
                            &mut emitted,
                            mapped + 1,
                        );
                    }
                }
                continue;
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => (old_index..old_index + old_len, 0..0),
            DiffOp::Insert {
                old_index,
                new_index,
                new_len,
            } => (old_index..old_index, new_index..new_index + new_len),
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => (
                old_index..old_index + old_len,
                new_index..new_index + new_len,
            ),
        };

        // The un-substitution case, and it is deliberately narrow: every
        // rendered line in the op must be one rendering produced, and there
        // must be exactly as many project lines to pair them with. A `Replace`
        // mixing verbatim and substituted lines falls through to the refusal
        // below — where it already was before ADR-022, so nothing regresses,
        // and the alternative is an emission ordering with no test to pin it.
        if let Some(lines) = lines
            && !deletes.is_empty()
            && deletes.len() == inserts.len()
            && deletes.clone().all(|r| !map.contains_key(&r))
            && let Some(reversed) = unsubstitute_run(
                &source_lines,
                &rendered_lines,
                &project_lines,
                &origins,
                deletes.clone(),
                inserts.clone(),
                lines,
            )
        {
            for (index, patched, expressions, rendered_line, project_line) in reversed {
                unsubstituted.push(Unsubstitution {
                    // Named by `backport`, which is the level that knows the
                    // file this line belongs to.
                    path: String::new(),
                    template_path: String::new(),
                    line: rendered_line + 1,
                    rendered: split_terminator(rendered_lines[rendered_line])
                        .0
                        .to_string(),
                    project: split_terminator(project_lines[project_line]).0.to_string(),
                    patched: split_terminator(&patched).0.to_string(),
                    expressions,
                });
                overrides.insert(index, patched);
            }
            continue;
        }

        for r in deletes.clone() {
            let Some(&mapped) = map.get(&r) else {
                return Err(BackportError::SubstitutedRegion {
                    path: path.to_string(),
                    template_path: source_path.to_string(),
                    line: r + 1,
                });
            };
            // Skip it: catch the source up to just before the mapped line,
            // then step over the line itself without emitting it.
            emit_through(&mut out, &source_lines, &overrides, &mut emitted, mapped);
            emitted = mapped + 1;
        }

        // An insertion has no rendered line of its own, so it is anchored on
        // the line it follows the deletions of. That line must be mapped, or we
        // would be inserting into a region the render produced.
        if !inserts.is_empty() {
            let anchor = deletes.end;
            if anchor < rendered_lines.len() && !map.contains_key(&anchor) {
                return Err(BackportError::SubstitutedRegion {
                    path: path.to_string(),
                    template_path: source_path.to_string(),
                    line: anchor + 1,
                });
            }
            if let Some(&mapped) = map.get(&anchor) {
                emit_through(&mut out, &source_lines, &overrides, &mut emitted, mapped);
            }
            for p in inserts {
                out.push(project_lines[p].to_string());
            }
        }
    }

    // Whatever the rendering dropped after the last mapped line.
    emit_through(
        &mut out,
        &source_lines,
        &overrides,
        &mut emitted,
        source_lines.len(),
    );

    Ok(Transposed {
        patched: out.concat(),
        unsubstituted,
    })
}

/// Put every reversal to the user, and refuse the file at the first decline.
///
/// Separate from `backport` so that it can be exercised with a gate that is not
/// a terminal. The rule it encodes is small and the consequence of getting it
/// wrong is not: an un-confirmed reversal is a change to what the template
/// produces for every project, shipped on nobody's authority.
fn confirm(
    unsubstitute: &mut Unsubstitute<'_>,
    reversals: &[Unsubstitution],
    path: &str,
    template_path: &str,
) -> Result<(), BackportError> {
    for reversal in reversals {
        let verdict = match unsubstitute {
            Unsubstitute::Always => Verdict::Accept,
            Unsubstitute::Ask(gate) => gate.confirm(&Proposal {
                path,
                template_path,
                line: reversal.line,
                rendered: &reversal.rendered,
                project: &reversal.project,
                patched: &reversal.patched,
                expressions: &reversal.expressions,
            }),
            // `lines` was `None`, so there is nothing to confirm.
            Unsubstitute::Never => Verdict::Accept,
        };
        if verdict == Verdict::Decline {
            return Err(BackportError::SubstitutedRegion {
                path: path.to_string(),
                template_path: template_path.to_string(),
                line: reversal.line,
            });
        }
    }
    Ok(())
}

/// Copy source lines up to `upto`, exclusive, honouring any line
/// un-substitution rewrote.
///
/// The one place a source line is emitted. There were four before, each with
/// its own copy of the same loop, and an override applied at three of them and
/// missed at the fourth is a patch that silently drops the user's edit.
fn emit_through(
    out: &mut Vec<String>,
    source_lines: &[&str],
    overrides: &BTreeMap<usize, String>,
    emitted: &mut usize,
    upto: usize,
) {
    while *emitted < upto {
        match overrides.get(emitted) {
            Some(patched) => out.push(patched.clone()),
            None => out.push(source_lines[*emitted].to_string()),
        }
        *emitted += 1;
    }
}

/// One reversal: the source line, its new text, the placeholders kept, and the
/// rendered and project lines it came from.
type Reversal = (usize, String, Vec<String>, usize, usize);

/// Reverse the substitutions across one aligned run, or refuse the whole run.
///
/// All or nothing on purpose. A run where one line reverses and the next does
/// not would emit half the user's change and drop the rest, and the round-trip
/// check would then refuse the file with `round_trip` — an honest failure, but
/// one that describes the wrong problem.
#[allow(clippy::too_many_arguments)]
fn unsubstitute_run(
    source_lines: &[&str],
    rendered_lines: &[&str],
    project_lines: &[&str],
    origins: &BTreeMap<usize, Range<usize>>,
    deletes: Range<usize>,
    inserts: Range<usize>,
    lines: &LineContext<'_>,
) -> Option<Vec<Reversal>> {
    let mut reversed = Vec::with_capacity(deletes.len());
    let mut claimed: Vec<usize> = Vec::with_capacity(deletes.len());

    for (r, p) in deletes.zip(inserts) {
        let (rendered_body, rendered_end) = split_terminator(rendered_lines[r]);
        let (project_body, project_end) = split_terminator(project_lines[p]);
        // A changed terminator is a line-ending conversion, not a content edit,
        // and carrying it as one would rewrite the whole file upstream.
        if rendered_end != project_end {
            return None;
        }

        let candidates = origins.get(&r)?.clone();
        let provenance = unsubstitute::pair(source_lines, candidates, rendered_body, lines)?;

        // A source line reproducing two different rendered lines is a loop
        // body, not a substitution: rewriting it would apply one iteration's
        // edit to every iteration.
        if claimed.contains(&provenance.source) {
            return None;
        }
        claimed.push(provenance.source);

        let patched =
            provenance.rewrite(source_lines[provenance.source], rendered_body, project_body)?;
        reversed.push((
            provenance.source,
            patched,
            provenance.expressions.clone(),
            r,
            p,
        ));
    }

    Some(reversed)
}

/// Render the patched source and require it to equal what the project has.
fn verify(
    patched: &str,
    project: &str,
    file: &Rendered,
    context: &Context,
    partials: &Arc<Partials>,
    undefined: Undefined,
    path: &str,
) -> Result<(), BackportError> {
    let refuse = || BackportError::RoundTrip {
        path: path.to_string(),
        template_path: file.source.clone(),
    };

    // A file copied byte-for-byte is not rendered, so its own bytes are the
    // rendering. Passing it through MiniJinja here would render a template the
    // real render never did.
    if !file.templated {
        return if patched == project {
            Ok(())
        } else {
            Err(refuse())
        };
    }

    let produced = render_string_with(patched, context, &file.source, partials, undefined)
        .map_err(|_| refuse())?;

    if produced == project {
        Ok(())
    } else {
        Err(refuse())
    }
}

/// Split keeping line terminators, so a file with no trailing newline stays
/// that way and `\r\n` survives (invariant 2 applies to what we emit, too).
pub(super) fn split_lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        match rest.find('\n') {
            Some(at) => {
                out.push(&rest[..=at]);
                rest = &rest[at + 1..];
            }
            None => {
                out.push(rest);
                rest = "";
            }
        }
    }
    out
}

fn line_count(text: &str) -> usize {
    split_lines(text).len()
}

fn counts(before: &str, after: &str) -> (usize, usize) {
    let diff = TextDiff::from_lines(before, after);
    let mut insertions = 0;
    let mut deletions = 0;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => insertions += 1,
            ChangeTag::Delete => deletions += 1,
            ChangeTag::Equal => {}
        }
    }
    (insertions, deletions)
}

/// A `git diff`-shaped section for one file.
///
/// Formatted here rather than by `GitBackend::diff_patch`, which needs two
/// *trees*: producing them would mean writing blobs and trees to answer a
/// question that reads nothing. `git tpl test` does the same, for the same
/// reason.
fn file_diff(path: &str, before: &str, after: &str, added: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!("diff --git a/{path} b/{path}\n"));
    if added {
        out.push_str("new file mode 100644\n");
        out.push_str("--- /dev/null\n");
    } else {
        out.push_str(&format!("--- a/{path}\n"));
    }
    out.push_str(&format!("+++ b/{path}\n"));

    let diff = TextDiff::from_lines(before, after);
    let mut unified = diff.unified_diff();
    unified.context_radius(3);
    // The `---`/`+++` pair is written above, in git's `a/`,`b/` form, which
    // `similar`'s header would duplicate in its own.
    //
    // `similar` emits `\ No newline at end of file` itself, so a file whose
    // last line has no terminator is already marked — and it must be, or
    // `git apply` silently adds one. Pinned by
    // `a_missing_final_newline_is_marked_in_the_patch`.
    out.push_str(&unified.to_string());
    out
}

/// Assemble the mailbox `git am` reads.
fn mailbox(
    project: &dyn GitBackend,
    project_root: &Path,
    revision_description: &str,
    files: &[BackportedFile],
    diffs: &[String],
) -> Result<String, OpError> {
    // The project's identity, because the project is where the work was done.
    // A backport is the user's patch; signing it with anything else would
    // misattribute it in the template's history.
    let name = project
        .config_string("user.name")?
        .unwrap_or_else(|| "git-tpl".to_string());
    let email = project
        .config_string("user.email")?
        .unwrap_or_else(|| "git-tpl@localhost".to_string());

    let project_name = project_root
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "the project".to_string());

    let mut out = String::new();
    // The magic date `git format-patch` uses to mark a synthesised mailbox.
    out.push_str("From 0000000000000000000000000000000000000000 Mon Sep 17 00:00:00 2001\n");
    out.push_str(&format!("From: {name} <{email}>\n"));
    out.push_str(&format!("Date: {}\n", rfc2822_now()));
    out.push_str(&format!(
        "Subject: [PATCH] tpl: backport from {project_name}\n\n"
    ));
    out.push_str(&format!(
        "Backported by git-tpl {} from {project_name}, rendered at {revision_description}.\n\n",
        crate::VERSION
    ));
    for file in files {
        out.push_str(&format!("  {} <- {}\n", file.source, file.rendered));
    }
    out.push('\n');
    for diff in diffs {
        out.push_str(diff);
    }
    out.push_str(&format!("-- \ngit-tpl {}\n\n", crate::VERSION));
    Ok(out)
}

/// The current time in the form a mailbox header wants.
///
/// The one clock read in this file, and it is metadata rather than content:
/// invariant 2 governs what a template *renders*, and the patch body is
/// byte-identical between two runs a second apart.
fn rfc2822_now() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    rfc2822(seconds)
}

/// A Unix timestamp as `Thu, 1 Jan 1970 00:00:00 +0000`.
///
/// Hand-rolled because the tree has no date crate and this is the only date it
/// formats; pulling one in for eight lines would be the larger cost. Always
/// UTC, so there is no zone database to be wrong about.
fn rfc2822(seconds: i64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let (hour, minute, second) = (rest / 3600, (rest % 3600) / 60, rest % 60);
    // 1970-01-01 was a Thursday, which is why `DAYS` starts there.
    let weekday = DAYS[days.rem_euclid(7) as usize];

    // Howard Hinnant's `civil_from_days`, with the era shifted so that the
    // leap-day lands at the end of the cycle and no special-casing is needed.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    // `mp` counts from March; `+ 2` and the wrap put January back at 0.
    let month = if mp < 10 { mp + 2 } else { mp - 10 };
    let year = era * 400 + yoe + i64::from(month < 2);

    format!(
        "{weekday}, {day} {} {year} {hour:02}:{minute:02}:{second:02} +0000",
        MONTHS[month as usize]
    )
}

/// The `git am` invocation git-tpl declines to run.
///
/// Built from the configured source, not from `Resolved::repo.workdir()`: a
/// resolved template is very often a throwaway clone in `/tmp`, and naming it
/// would send the user to apply their patch into a directory that is about to
/// be deleted.
fn apply_command(source: &str, project_root: &Path) -> String {
    let target = match super::resolve::local_path(source) {
        Some(path) => relative_to(&path, project_root),
        // A URL has no local clone to name, so the placeholder says so rather
        // than pretending to know.
        None => "<your-template-clone>".to_string(),
    };
    format!("git tpl backport | git -C {target} am")
}

/// A short spelling of `path` from `from`, falling back to the absolute form.
///
/// Walks up as well as down, because the overwhelmingly common layout is the
/// template beside the project rather than inside it, and `/tmp/w/tpl` is a
/// worse thing to paste than `../tpl`.
fn relative_to(path: &Path, from: &Path) -> String {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let base = std::fs::canonicalize(from).unwrap_or_else(|_| from.to_path_buf());

    let mut target_parts = target.components();
    let mut base_parts = base.components();
    let mut common = 0;
    while let (Some(a), Some(b)) = (target_parts.clone().next(), base_parts.clone().next()) {
        if a != b {
            break;
        }
        target_parts.next();
        base_parts.next();
        common += 1;
    }

    // Nothing in common means different roots — on Windows, different drives.
    // There is no relative spelling, and inventing one would be wrong.
    if common == 0 {
        return slashed(&target);
    }

    let mut out = std::path::PathBuf::new();
    for _ in base_parts {
        out.push("..");
    }
    for part in target_parts {
        out.push(part);
    }

    let relative = slashed(&out);
    if relative.is_empty() {
        return ".".to_string();
    }
    // A path with no `..` still needs a leading `./` to read as a path rather
    // than as a remote name.
    if relative.starts_with("..") {
        relative
    } else {
        format!("./{relative}")
    }
}

/// A path spelled with `/`, on every platform.
///
/// The result is printed to be pasted into a shell. Git accepts forward
/// slashes on Windows, and in Git Bash — the shell a Windows user most likely
/// has open — a backslash is an escape character, so `git -C ..\tpl am` is not
/// the command it looks like.
fn slashed(path: &Path) -> String {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// `<root>/<path>`, the path as the template repository knows it.
fn join_root(root: &str, path: &str) -> String {
    let root = root.trim_matches('/');
    if root.is_empty() {
        path.to_string()
    } else {
        format!("{root}/{path}")
    }
}

fn excluded(path: &str, exclude: &[String]) -> bool {
    exclude.iter().any(|pattern| glob_matches(pattern, path))
}

/// A `fnmatch`-shaped match, where `*` does not cross a `/` and `**` does.
///
/// Deliberately not the pathspec matcher libgit2 applies to `paths`: those are
/// *inclusions*, where Git's own semantics are what a user expects, and this
/// is an exclusion list git-tpl owns.
fn glob_matches(pattern: &str, path: &str) -> bool {
    // A bare name matches at any depth, which is what `--exclude Cargo.lock`
    // obviously means.
    if !pattern.contains('/') && !pattern.contains('*') && path.rsplit('/').next() == Some(pattern)
    {
        return true;
    }
    glob_rec(pattern.as_bytes(), path.as_bytes())
}

fn glob_rec(pattern: &[u8], path: &[u8]) -> bool {
    if pattern.is_empty() {
        return path.is_empty();
    }
    if pattern.starts_with(b"**") {
        let rest = &pattern[2..];
        let rest = rest.strip_prefix(b"/").unwrap_or(rest);
        for at in 0..=path.len() {
            if glob_rec(rest, &path[at..]) {
                return true;
            }
        }
        return false;
    }
    if pattern[0] == b'*' {
        for at in 0..=path.len() {
            // A single `*` stops at a separator.
            if path[..at].contains(&b'/') {
                break;
            }
            if glob_rec(&pattern[1..], &path[at..]) {
                return true;
            }
        }
        return false;
    }
    if !path.is_empty() && (pattern[0] == b'?' || pattern[0] == path[0]) {
        return glob_rec(&pattern[1..], &path[1..]);
    }
    false
}

fn read_required(project: &dyn GitBackend, tree: Oid, path: &str) -> Result<Vec<u8>, OpError> {
    project.read_path(tree, path)?.ok_or_else(|| {
        BackportError::UnknownPath {
            path: path.to_string(),
        }
        .into()
    })
}

fn to_text(bytes: &[u8], path: &str, template_path: &str) -> Result<String, BackportError> {
    String::from_utf8(bytes.to_vec()).map_err(|_| BackportError::Binary {
        path: path.to_string(),
        template_path: template_path.to_string(),
    })
}

/// The same sniff `render` uses, so "binary" means one thing in this tree.
fn is_binary(content: &[u8]) -> bool {
    content.iter().take(8000).any(|byte| *byte == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(0, "Thu, 1 Jan 1970 00:00:00 +0000")]
    #[case(1_000_000_000, "Sun, 9 Sep 2001 01:46:40 +0000")]
    // A leap day, which is the case the era arithmetic exists for.
    #[case(1_709_164_800, "Thu, 29 Feb 2024 00:00:00 +0000")]
    #[case(1_735_689_599, "Tue, 31 Dec 2024 23:59:59 +0000")]
    fn a_timestamp_formats_as_an_rfc2822_date(#[case] seconds: i64, #[case] expected: &str) {
        assert_eq!(rfc2822(seconds), expected);
    }

    #[rstest]
    #[case("Cargo.lock", "Cargo.lock", true)]
    // A bare name matches at any depth, because that is what a user writing
    // `--exclude Cargo.lock` plainly means.
    #[case("Cargo.lock", "nested/Cargo.lock", true)]
    #[case("*.md", "README.md", true)]
    // A single star does not cross a separator.
    #[case("*.md", "docs/README.md", false)]
    #[case("**/*.md", "docs/deep/README.md", true)]
    #[case("docs/*", "docs/README.md", true)]
    #[case("docs/*", "docs/a/b.md", false)]
    #[case("src/**", "src/a/b.rs", true)]
    #[case("*.md", "README.mdx", false)]
    // A `**` that matches nothing still has to say so.
    #[case("**/*.md", "docs/deep/README.txt", false)]
    #[case("docs/**", "src/a.rs", false)]
    fn a_glob_matches_what_a_user_would_expect(
        #[case] pattern: &str,
        #[case] path: &str,
        #[case] expected: bool,
    ) {
        assert_eq!(glob_matches(pattern, path), expected);
    }

    #[test]
    fn lines_keep_their_terminators_so_a_missing_final_newline_survives() {
        assert_eq!(split_lines("a\nb"), vec!["a\n", "b"]);
        assert_eq!(split_lines("a\nb\n"), vec!["a\n", "b\n"]);
        assert_eq!(split_lines("a\r\n"), vec!["a\r\n"]);
        assert_eq!(split_lines(""), Vec::<&str>::new());
    }

    #[test]
    fn a_change_to_a_verbatim_line_lands_on_the_source_line() {
        // The substituted line is untouched, so it must come back out of the
        // patched source with its placeholder intact.
        let source = "# {{ name }}\n\nA project.\n";
        let rendered = "# acme\n\nA project.\n";
        let project = "# acme\n\nA fine project.\n";

        let patched = transpose(
            source,
            rendered,
            project,
            "README.md",
            "README.md.jinja",
            None,
        )
        .unwrap()
        .patched;
        assert_eq!(patched, "# {{ name }}\n\nA fine project.\n");
    }

    #[test]
    fn an_insertion_between_verbatim_lines_is_placed_in_the_source() {
        let source = "# {{ name }}\n\nA project.\n";
        let rendered = "# acme\n\nA project.\n";
        let project = "# acme\n\nA project.\nAnd more.\n";

        let patched = transpose(
            source,
            rendered,
            project,
            "README.md",
            "README.md.jinja",
            None,
        )
        .unwrap()
        .patched;
        assert_eq!(patched, "# {{ name }}\n\nA project.\nAnd more.\n");
    }

    #[test]
    fn editing_a_substituted_line_is_refused_rather_than_guessed() {
        // The user renamed the project in the rendered file. Reversing that
        // into `{{ name }}` would be a guess, and a wrong one: they meant to
        // change their answer, not the template.
        let source = "# {{ name }}\n\nA project.\n";
        let rendered = "# acme\n\nA project.\n";
        let project = "# widgets\n\nA project.\n";

        let error = transpose(
            source,
            rendered,
            project,
            "README.md",
            "README.md.jinja",
            None,
        )
        .unwrap_err();
        std::assert_matches!(error, BackportError::SubstitutedRegion { line: 1, .. });
    }

    #[test]
    fn a_deletion_of_a_verbatim_line_removes_that_source_line() {
        let source = "{{ name }}\nkeep\ndrop\n";
        let rendered = "acme\nkeep\ndrop\n";
        let project = "acme\nkeep\n";

        let patched = transpose(source, rendered, project, "f", "f.jinja", None)
            .unwrap()
            .patched;
        assert_eq!(patched, "{{ name }}\nkeep\n");
    }

    /// An insertion after a collapsed region lands below it, not above it.
    ///
    /// The rendering dropped the `{% if %}`, so the source line the insert
    /// anchors on is four lines further down than the rendered one. Getting
    /// this wrong puts the user's new line *inside* the conditional, where it
    /// would appear for some projects and not others.
    #[test]
    fn an_insertion_after_a_collapsed_region_is_placed_below_it() {
        let source = "one\n{% if extra %}\ngone\n{% endif %}\ntwo\n";
        let rendered = "one\ntwo\n";
        let project = "one\nadded\ntwo\n";

        let patched = transpose(source, rendered, project, "f", "f.jinja", None)
            .unwrap()
            .patched;
        assert_eq!(
            patched,
            "one\n{% if extra %}\ngone\n{% endif %}\nadded\ntwo\n"
        );
    }

    /// A deletion after a collapsed region catches the source up first.
    ///
    /// Without the catch-up the `{% if %}` between the last mapped line and
    /// the deleted one is swallowed along with it, and the template stops
    /// parsing — a failure that would reach every downstream project.
    #[test]
    fn a_deletion_after_a_collapsed_region_keeps_the_region() {
        let source = "one\n{% if extra %}\ngone\n{% endif %}\ndrop\n";
        let rendered = "one\ndrop\n";
        let project = "one\n";

        let patched = transpose(source, rendered, project, "f", "f.jinja", None)
            .unwrap()
            .patched;
        assert_eq!(patched, "one\n{% if extra %}\ngone\n{% endif %}\n");
    }

    #[test]
    fn a_collapsed_conditional_keeps_its_source_lines() {
        // `{% if %}` produced nothing, so the source has lines the rendering
        // does not. A change elsewhere must not delete them.
        let source = "one\n{% if extra %}\nextra\n{% endif %}\ntwo\n";
        let rendered = "one\n\ntwo\n";
        let project = "one\n\ntwo point five\n";

        let patched = transpose(source, rendered, project, "f", "f.jinja", None)
            .unwrap()
            .patched;
        assert!(patched.contains("{% if extra %}"), "{patched}");
        assert!(patched.contains("two point five\n"), "{patched}");
    }

    #[rstest]
    #[case("", "a/b.md", "a/b.md")]
    #[case("template", "a/b.md", "template/a/b.md")]
    #[case("/template/", "a/b.md", "template/a/b.md")]
    fn a_render_root_prefixes_the_patch_path(
        #[case] root: &str,
        #[case] path: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(join_root(root, path), expected);
    }

    /// An insertion adjacent to a rendered line has no anchor in the source.
    ///
    /// The `Delete` path is covered above; this is the other way in, and it is
    /// the one a user hits by adding a line right under a substituted heading.
    #[test]
    fn an_insertion_against_a_substituted_line_is_refused() {
        let source = "# {{ name }}\n{{ tagline }}\n";
        let rendered = "# acme\na fine thing\n";
        let project = "# acme\nadded\na fine thing\n";

        let error = transpose(
            source,
            rendered,
            project,
            "README.md",
            "README.md.jinja",
            None,
        )
        .unwrap_err();
        std::assert_matches!(error, BackportError::SubstitutedRegion { line: 2, .. });
    }

    /// Source lines the rendering dropped after the last mapped line survive.
    ///
    /// The tail of a file is the easiest place to lose a `{% endif %}`, and
    /// losing one produces a template that no longer parses.
    #[test]
    fn a_trailing_collapsed_region_is_kept() {
        let source = "keep\n{% if extra %}\ngone\n{% endif %}\n";
        let rendered = "keep\n";
        let project = "changed\n";

        let patched = transpose(source, rendered, project, "f", "f.jinja", None)
            .unwrap()
            .patched;
        assert_eq!(patched, "changed\n{% if extra %}\ngone\n{% endif %}\n");
    }

    /// A file with no trailing newline keeps that property through the patch.
    ///
    /// The marker comes from `similar`, not from us — this pins that it is
    /// still there, because without it `git apply` silently appends a newline
    /// the user never wrote.
    #[test]
    fn a_missing_final_newline_is_marked_in_the_patch() {
        let diff = file_diff("f", "a\nb\n", "a\nc", false);
        assert!(diff.ends_with("\\ No newline at end of file\n"), "{diff}");
    }

    // ---- un-substitution, ADR-022 -----------------------------------------

    /// The context `# {{ name }}` and friends are rendered against.
    fn answers(pairs: &[(&str, &str)]) -> Context {
        let mut context = Context::default();
        for (name, value) in pairs {
            context.set_answer(*name, crate::template::Value::String((*value).to_string()));
        }
        context
    }

    /// `transpose` with un-substitution on.
    fn reverse(
        source: &str,
        rendered: &str,
        project: &str,
        context: &Context,
    ) -> Result<Transposed, BackportError> {
        let lines = LineContext {
            context,
            partials: crate::eval::no_partials(),
            undefined: Undefined::Lenient,
        };
        transpose(source, rendered, project, "f", "f.jinja", Some(&lines))
    }

    /// The acceptance case: the change is beside the placeholder, not in it.
    #[test]
    fn an_edit_beside_a_substitution_keeps_the_placeholder() {
        let context = answers(&[("name", "acme")]);
        let out = reverse(
            "# {{ name }} — a service\n",
            "# acme — a service\n",
            "# acme — a web service\n",
            &context,
        )
        .unwrap();

        assert_eq!(out.patched, "# {{ name }} — a web service\n");
        assert_eq!(out.unsubstituted.len(), 1);
        assert_eq!(out.unsubstituted[0].line, 1);
        assert_eq!(out.unsubstituted[0].expressions, vec!["{{ name }}"]);
    }

    /// Replacing the whole value is a change of *answer*, not of template.
    #[test]
    fn replacing_the_value_itself_is_still_refused() {
        let context = answers(&[("name", "acme")]);
        let error = reverse("# {{ name }}\n", "# acme\n", "# widgets\n", &context).unwrap_err();
        std::assert_matches!(error, BackportError::SubstitutedRegion { line: 1, .. });
    }

    /// The case ADR-020 says a substitution table cannot get right.
    ///
    /// `author` happens to be the month the template hard-codes. Nothing here
    /// searches for the value, so the literal " in June." is just literal text
    /// and edits to it carry cleanly.
    #[test]
    fn a_value_that_coincides_with_literal_text_is_not_confused_with_it() {
        let context = answers(&[("author", "June")]);
        let out = reverse(
            "Written by {{ author }} in June.\n",
            "Written by June in June.\n",
            "Written by June in July.\n",
            &context,
        )
        .unwrap();
        assert_eq!(out.patched, "Written by {{ author }} in July.\n");
        assert_eq!(out.unsubstituted.len(), 1);
    }

    /// The other half of the same line: editing the *author* is an answer change.
    #[test]
    fn editing_the_value_of_a_coinciding_name_is_refused() {
        let context = answers(&[("author", "June")]);
        let error = reverse(
            "Written by {{ author }} in June.\n",
            "Written by June in June.\n",
            "Written by Ada in June.\n",
            &context,
        )
        .unwrap_err();
        std::assert_matches!(error, BackportError::SubstitutedRegion { .. });
    }

    /// The patch that round-trips and is still wrong.
    ///
    /// `.0` appended to a rendered version sits against the value, and placing
    /// it in the literal gives `{{ version }}.0` — correct for this user and
    /// wrong for every other project. The slider refuses it.
    #[test]
    fn an_edit_that_could_have_slid_into_a_value_is_refused() {
        let context = answers(&[("version", "1.0")]);
        let error = reverse(
            "version = \"{{ version }}\"\n",
            "version = \"1.0\"\n",
            "version = \"1.0.0\"\n",
            &context,
        )
        .unwrap_err();
        std::assert_matches!(error, BackportError::SubstitutedRegion { .. });
    }

    /// A value rendering to nothing has a zero-width range, which the
    /// reassembly cannot confirm and no edit can be attributed near.
    #[test]
    fn a_line_with_an_empty_value_is_refused() {
        let context = answers(&[("suffix", ""), ("name", "acme")]);
        let error = reverse(
            "# {{ name }}{{ suffix }} here\n",
            "# acme here\n",
            "# acme there\n",
            &context,
        )
        .unwrap_err();
        std::assert_matches!(error, BackportError::SubstitutedRegion { .. });
    }

    /// A loop body has no line-local provenance: the source line renders
    /// against a binding that is not in the context at all.
    #[test]
    fn a_line_inside_a_loop_is_refused() {
        let context = answers(&[("name", "acme")]);
        let error = reverse(
            "{% for item in items %}\n- {{ item }} ok\n{% endfor %}\n",
            "- one ok\n- two ok\n",
            "- one fine\n- two ok\n",
            &context,
        )
        .unwrap_err();
        std::assert_matches!(error, BackportError::SubstitutedRegion { .. });
    }

    /// Two placeholders on a line, and an edit between them.
    #[test]
    fn an_edit_between_two_placeholders_is_carried() {
        let context = answers(&[("tool", "cargo"), ("task", "test")]);
        let out = reverse(
            "run {{ tool }} then {{ task }}\n",
            "run cargo then test\n",
            "run cargo and then test\n",
            &context,
        )
        .unwrap();
        assert_eq!(out.patched, "run {{ tool }} and then {{ task }}\n");
    }

    /// Appending to a line that ends in a placeholder works, which needs the
    /// zero-width trailing literal the scanner emits.
    #[test]
    fn appending_after_a_trailing_placeholder_is_carried() {
        let context = answers(&[("tool", "cargo")]);
        let out = reverse(
            "run {{ tool }}\n",
            "run cargo\n",
            "run cargo --release\n",
            &context,
        )
        .unwrap();
        assert_eq!(out.patched, "run {{ tool }} --release\n");
    }

    /// Whitespace control reaches into the neighbouring line, so a byte range
    /// computed on this one would describe the wrong text.
    #[test]
    fn a_line_with_whitespace_control_is_refused() {
        let context = answers(&[("name", "acme")]);
        let error = reverse("x {{- name }} y\n", "xacme y\n", "xacme z\n", &context).unwrap_err();
        std::assert_matches!(error, BackportError::SubstitutedRegion { .. });
    }

    /// Off by default: nothing changes for a caller that does not opt in.
    #[test]
    fn without_a_line_context_nothing_is_reversed() {
        let error = transpose(
            "# {{ name }}\n",
            "# acme\n",
            "# acme!\n",
            "f",
            "f.jinja",
            None,
        )
        .unwrap_err();
        std::assert_matches!(error, BackportError::SubstitutedRegion { line: 1, .. });
    }

    /// A gate that answers the same way every time, and counts.
    struct Fake {
        verdict: Verdict,
        asked: Vec<usize>,
    }

    impl crate::ops::Unsubstituter for Fake {
        fn confirm(&mut self, proposal: &Proposal<'_>) -> Verdict {
            self.asked.push(proposal.line);
            self.verdict
        }
    }

    fn reversal(line: usize) -> Unsubstitution {
        Unsubstitution {
            path: String::new(),
            template_path: String::new(),
            line,
            rendered: "# acme".to_string(),
            project: "# acme!".to_string(),
            patched: "# {{ name }}!".to_string(),
            expressions: vec!["{{ name }}".to_string()],
        }
    }

    #[test]
    fn every_reversal_is_put_to_the_user() {
        let mut gate = Fake {
            verdict: Verdict::Accept,
            asked: Vec::new(),
        };
        let reversals = [reversal(1), reversal(4)];
        confirm(
            &mut Unsubstitute::Ask(&mut gate),
            &reversals,
            "README.md",
            "README.md.jinja",
        )
        .unwrap();
        assert_eq!(gate.asked, vec![1, 4]);
    }

    /// Declining one line refuses the file, and stops asking about the rest.
    ///
    /// The patch is per file, so a partial acceptance would have to drop the
    /// declined line and keep the others — which is a patch the user never saw.
    #[test]
    fn declining_a_reversal_refuses_the_file_by_name() {
        let mut gate = Fake {
            verdict: Verdict::Decline,
            asked: Vec::new(),
        };
        let reversals = [reversal(2), reversal(7)];
        let error = confirm(
            &mut Unsubstitute::Ask(&mut gate),
            &reversals,
            "README.md",
            "README.md.jinja",
        )
        .unwrap_err();

        std::assert_matches!(error, BackportError::SubstitutedRegion { line: 2, .. });
        assert_eq!(gate.asked, vec![2], "it kept asking after a refusal");
    }

    /// `--unsubstitute` is the decision taken in advance, so nothing is asked.
    ///
    /// `Never` is here too, for the same call: it means no reversal was ever
    /// attempted, so the list is empty in practice — and a variant that would
    /// refuse a line the caller never offered is not the behaviour wanted if
    /// that ever stops being true.
    #[rstest]
    #[case(Unsubstitute::Always)]
    #[case(Unsubstitute::Never)]
    fn a_decision_taken_in_advance_asks_nothing(#[case] mut mode: Unsubstitute<'static>) {
        let reversals = [reversal(1)];
        confirm(&mut mode, &reversals, "README.md", "README.md.jinja").unwrap();
    }

    /// A line-ending conversion is not a content edit, and carrying it as one
    /// would rewrite the whole file upstream.
    #[test]
    fn a_changed_line_terminator_is_not_un_substituted() {
        let context = answers(&[("name", "acme")]);
        let error = reverse(
            "# {{ name }} — a service\n",
            "# acme — a service\n",
            "# acme — a web service\r\n",
            &context,
        )
        .unwrap_err();
        std::assert_matches!(error, BackportError::SubstitutedRegion { .. });
    }

    /// One source line reproducing two rendered lines is a loop, and rewriting
    /// it would apply one iteration's edit to every iteration.
    #[test]
    fn a_source_line_claimed_twice_is_refused() {
        let context = answers(&[("name", "acme")]);
        // The rendering repeated the line, so both rendered lines have the same
        // and only candidate.
        let error = reverse(
            "> {{ name }} here\n",
            "> acme here\n> acme here\n",
            "> acme there\n> acme everywhere\n",
            &context,
        )
        .unwrap_err();
        std::assert_matches!(error, BackportError::SubstitutedRegion { .. });
    }

    /// A template beside its own project is `.`, not the empty string.
    #[test]
    fn a_template_that_is_the_project_is_a_dot() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(relative_to(dir.path(), dir.path()), ".");
    }

    #[test]
    fn a_url_source_has_no_local_clone_to_name() {
        let command = apply_command("https://example.com/t.git", Path::new("/tmp"));
        assert!(command.contains("<your-template-clone>"), "{command}");
    }

    #[test]
    fn a_sibling_template_is_named_relatively() {
        let dir = tempfile::tempdir().unwrap();
        let template = dir.path().join("my-template");
        let project = dir.path().join("my-service");
        std::fs::create_dir_all(&template).unwrap();
        std::fs::create_dir_all(&project).unwrap();

        // The common layout, and the one an absolute path serves worst.
        assert_eq!(relative_to(&template, &project), "../my-template");
    }

    #[test]
    fn a_nested_template_keeps_its_leading_dot() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_path_buf();
        let template = project.join("vendor/tpl");
        std::fs::create_dir_all(&template).unwrap();

        assert_eq!(relative_to(&template, &project), "./vendor/tpl");
    }

    /// The hint is pasted into a shell, and on Windows the native separator is
    /// an escape character in the shell most likely to be open.
    #[test]
    fn a_path_in_the_hint_is_spelled_with_forward_slashes() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("a/project");
        let template = dir.path().join("b/template");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&template).unwrap();

        let spelled = relative_to(&template, &project);
        assert!(!spelled.contains('\\'), "{spelled}");
        assert_eq!(spelled, "../../b/template");
    }
}
