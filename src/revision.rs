//! Shared formatting of a requested template reference and its resolved commit.

use crate::git::Oid;

/// The reference name used when rendering an uncommitted working tree.
pub(crate) const WORKTREE_REF: &str = "<worktree>";

/// Format a revision for diagnostics and output without depending on ops.
pub(crate) fn describe_revision(reference: &str, revision: Oid) -> String {
    if reference == WORKTREE_REF {
        format!("{} (+ uncommitted changes)", revision.short())
    } else {
        format!("{reference} ({})", revision.short())
    }
}
