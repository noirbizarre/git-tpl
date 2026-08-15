//! Process exit codes.
//!
//! Defined once, here, and used by both `main` and the commands that return a
//! code. Two copies that a test asserts are equal is a worse design than one
//! copy that cannot disagree.

/// The command did what was asked.
pub const SUCCESS: u8 = 0;

/// The command failed.
pub const FAILURE: u8 = 1;

/// Nothing failed, but something is outstanding — the template moved, or there
/// is a rendering that has not been merged.
///
/// A distinct code rather than a second failure, so that `git tpl status` can
/// be used as a drift check in CI:
///
/// ```sh
/// git tpl status --quiet || echo "template drift detected"
/// ```
pub const PENDING: u8 = 2;

#[cfg(test)]
mod tests {
    use super::*;

    /// Collapsing PENDING into FAILURE would make the drift check useless,
    /// which is the entire reason it is separate.
    #[test]
    fn pending_is_not_the_same_code_as_failure() {
        let codes = [SUCCESS, FAILURE, PENDING];
        let unique: std::collections::BTreeSet<_> = codes.iter().collect();
        assert_eq!(unique.len(), codes.len(), "{codes:?}");
    }
}
