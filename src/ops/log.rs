//! Source history bounded by the rendering actually reachable from the project's HEAD.

use std::path::Path;
use std::sync::LazyLock;

use miette::Diagnostic;
use regex_lite::Regex;
use thiserror::Error;

use super::{OpError, identify, require_tip, resolve};
use crate::git::{GitBackend, HistoryEntry, Oid};
use crate::provenance::{Provenance, WORKTREE_REF};
use crate::refs::TemplateId;

/// Failures specific to locating an exact applied source-history boundary.
#[derive(Debug, Error, Diagnostic)]
pub enum LogError {
    /// Only a single revision or one two-dot range is meaningful here.
    #[error("invalid template history range `{reference}`")]
    #[diagnostic(
        code(tpl::log::invalid_range),
        help(
            "use REF or A..B; omitted range endpoints mean the applied template revision; three-dot ranges are unsupported"
        )
    )]
    InvalidRange {
        /// The expression the user supplied.
        reference: String,
    },
    /// No rendering is reachable from this branch.
    #[error("no rendering of `{ref_name}` has been applied to HEAD")]
    #[diagnostic(
        code(tpl::log::not_applied),
        help(
            "merge the rendered template with `git tpl merge` first; squash and cherry-pick do not preserve template ancestry"
        )
    )]
    NotApplied {
        /// The rendered ref being inspected.
        ref_name: String,
    },
    /// An applied commit cannot identify its source history.
    #[error("applied rendering {commit} has no usable template source provenance")]
    #[diagnostic(
        code(tpl::log::provenance),
        help(
            "inspect `git show {commit}`; restore the original rendered history or merge a new rendering with valid Template-Source and Template-Commit provenance"
        )
    )]
    Provenance {
        /// The exact applied rendering, not a guessed older baseline.
        commit: Oid,
    },
    /// Uncommitted input has no exact commit boundary.
    #[error("applied rendering {commit} was made from an uncommitted template worktree")]
    #[diagnostic(
        code(tpl::log::dirty),
        help(
            "commit the template changes, then update and merge a clean rendering to establish an exact source-history boundary"
        )
    )]
    Dirty {
        /// The dirty rendering.
        commit: Oid,
    },
    /// Rewritten or incomplete source history no longer contains the baseline.
    #[error("applied source commit {commit} is unavailable in `{origin}`")]
    #[diagnostic(
        code(tpl::log::missing_commit),
        help(
            "restore the historical commit in the template repository, or unshallow a local source clone; reason: {reason}"
        )
    )]
    MissingCommit {
        /// The historical source commit.
        commit: Oid,
        /// Its repository.
        origin: String,
        /// The backend failure.
        reason: String,
    },
}

/// A conventional subject prefix; no commits are excluded when parsing fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conventional {
    /// The type as written, including unknown types.
    pub kind: String,
    /// Optional scope without parentheses.
    pub scope: Option<String>,
    /// Whether the subject explicitly carries `!`.
    pub breaking: bool,
    /// Byte length of the prefix, including the colon and following space.
    pub prefix_len: usize,
}

/// Parse presentation metadata without rewriting the original subject.
pub fn conventional(subject: &str) -> Option<Conventional> {
    static PREFIX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^([A-Za-z][A-Za-z0-9_-]*)(\(([^()\r\n]+)\))?(!)?: ")
            .expect("fixed conventional prefix")
    });
    let captures = PREFIX.captures(subject)?;
    Some(Conventional {
        kind: captures[1].to_string(),
        scope: captures.get(3).map(|scope| scope.as_str().to_string()),
        breaking: captures.get(4).is_some(),
        prefix_len: captures.get(0)?.end(),
    })
}

/// The main source's history, with endpoints kept distinct from rendered commits.
pub struct Log {
    /// Template identity in the consuming project.
    pub id: TemplateId,
    /// The append-only rendered ref.
    pub ref_name: String,
    /// Repository recorded by the applied rendering.
    pub source: String,
    /// The applied project-side rendering.
    pub applied_rendering: Oid,
    /// Reference recorded in provenance, when available.
    pub applied_reference: Option<String>,
    /// The applied source commit.
    pub applied_revision: Oid,
    /// Selection syntax, independent of whether either endpoint was omitted.
    pub mode: LogMode,
    /// Included endpoint name; absent when it defaults to applied.
    pub target_reference: Option<String>,
    /// Target source commit (the applied commit in default mode).
    pub target_revision: Oid,
    /// Excluded endpoint name; absent when it defaults to applied or there is no range.
    pub exclude_reference: Option<String>,
    /// Excluded source commit; absent outside range mode.
    pub exclude_revision: Option<Oid>,
    /// Whether an explicit target descends from the applied source commit.
    pub target_descends_from_applied: bool,
    /// Effective limit; absent means unlimited.
    pub max_count: Option<usize>,
    /// Ordered source commits.
    pub entries: Vec<HistoryEntry>,
}

/// The syntax determines whether history has an excluded endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogMode {
    /// No argument: history through the merged source revision.
    Applied,
    /// One explicit revision: its complete reachable history.
    History,
    /// Two-dot reachability subtraction.
    Range,
}

fn selection(reference: Option<&str>) -> Result<(LogMode, Option<&str>, Option<&str>), LogError> {
    let Some(reference) = reference else {
        return Ok((LogMode::Applied, None, None));
    };
    if let Some((left, right)) = reference.split_once("..") {
        // A third dot or another separator must not silently become part of an endpoint.
        if left.ends_with('.') || right.starts_with('.') || right.contains("..") {
            return Err(LogError::InvalidRange {
                reference: reference.into(),
            });
        }
        return Ok((
            LogMode::Range,
            (!right.is_empty()).then_some(right),
            (!left.is_empty()).then_some(left),
        ));
    }
    Ok((LogMode::History, Some(reference), None))
}

/// Inspect source history without rendering or changing any project state.
pub fn log(
    project: &dyn GitBackend,
    project_root: &Path,
    reference: Option<&str>,
    max_count: Option<usize>,
    access: &crate::git::cache::Access,
) -> Result<Log, OpError> {
    let (mode, target_reference, exclude_reference) = selection(reference)?;
    let (id, ref_name) = identify(project_root)?;
    let tip = require_tip(project, &ref_name)?;
    let not_applied = || LogError::NotApplied {
        ref_name: ref_name.clone(),
    };
    let head = project.head_commit()?.ok_or_else(not_applied)?;
    let mut candidate = tip;
    let applied = loop {
        let commit = project.commit(candidate)?;
        if project.is_ancestor(candidate, head)? {
            break commit;
        }
        // Renderings append along the first parent; other parents may be unrelated user history.
        candidate = *commit.parents.first().ok_or_else(not_applied)?;
    };
    let invalid = || LogError::Provenance {
        commit: applied.oid,
    };
    let recorded = Provenance::parse(&applied.message).ok_or_else(invalid)?;
    if recorded.dirty
        || recorded.reference.as_deref() == Some(WORKTREE_REF)
        || recorded.revision == Some(Oid::from_bytes([0; 20]))
    {
        return Err(LogError::Dirty {
            commit: applied.oid,
        }
        .into());
    }
    let source = recorded
        .source
        .filter(|source| !source.trim().is_empty())
        .ok_or_else(invalid)?;
    let applied_revision = recorded.revision.ok_or_else(invalid)?;
    // History remains useful before template.toml existed, or while its latest version is invalid.
    let location = crate::config::source_location(&source, project_root);
    let applied_id = applied_revision.to_hex();
    let first_reference = target_reference.or(exclude_reference);
    let opened = resolve::open_repository(
        &location,
        first_reference.or(Some(&applied_id)),
        false,
        access,
    )?;
    let repo = &opened.repo;
    repo.commit(applied_revision)
        .map_err(|error| LogError::MissingCommit {
            commit: applied_revision,
            origin: source.clone(),
            reason: error.to_string(),
        })?;
    let resolve_endpoint =
        |opened: &resolve::OpenedRepository, reference: &str| match &opened.resolved {
            Some((_, revision)) => Ok(*revision),
            None => opened.repo.resolve_revision(reference, &source),
        };
    let first_revision = first_reference
        .map(|reference| resolve_endpoint(&opened, reference))
        .transpose()?;
    // A second mutable endpoint must be frozen under the cache lock too, not resolved after a fetch races us.
    // Keep both leases alive until the walk finishes, including when the endpoints name disjoint branches.
    let second = match (target_reference, exclude_reference) {
        (Some(target), Some(exclude)) if target != exclude => Some(resolve::open_repository(
            &location,
            Some(exclude),
            false,
            access,
        )?),
        _ => None,
    };
    let target_revision = target_reference
        .and(first_revision)
        .unwrap_or(applied_revision);
    let exclude_revision = if mode == LogMode::Range {
        Some(match (exclude_reference, &second) {
            (Some(reference), Some(opened)) => resolve_endpoint(opened, reference)?,
            (Some(_), None) => first_revision.expect("explicit endpoint was resolved"),
            (None, _) => applied_revision,
        })
    } else {
        None
    };
    let target_descends_from_applied = repo.is_ancestor(applied_revision, target_revision)?;
    let entries = repo.history(target_revision, exclude_revision, max_count)?;
    Ok(Log {
        id,
        ref_name,
        source,
        applied_rendering: applied.oid,
        applied_reference: recorded.reference,
        applied_revision,
        mode,
        target_reference: target_reference.map(str::to_string),
        target_revision,
        exclude_reference: exclude_reference.map(str::to_string),
        exclude_revision,
        target_descends_from_applied,
        max_count,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conventional_prefixes_preserve_scopes_unknown_types_and_breaking_markers() {
        for (subject, kind, scope, breaking, rest) in [
            (
                "feat: support history",
                "feat",
                None,
                false,
                "support history",
            ),
            (
                "fix(cli): preserve Unicode café",
                "fix",
                Some("cli"),
                false,
                "preserve Unicode café",
            ),
            (
                "custom-type(scope)!: change contract",
                "custom-type",
                Some("scope"),
                true,
                "change contract",
            ),
            (
                "refactor!: change contract",
                "refactor",
                None,
                true,
                "change contract",
            ),
        ] {
            let parsed = conventional(subject).expect("conventional prefix");
            assert_eq!(parsed.kind, kind);
            assert_eq!(parsed.scope.as_deref(), scope);
            assert_eq!(parsed.breaking, breaking);
            assert_eq!(&subject[parsed.prefix_len..], rest);
        }
        for subject in [
            "Merge branch main",
            "plain subject",
            "fix(no closing: text",
            "feat:missing space",
        ] {
            assert!(conventional(subject).is_none(), "{subject}");
        }
    }
}
