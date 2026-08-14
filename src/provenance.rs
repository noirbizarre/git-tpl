//! Provenance, recorded as commit trailers on the rendered commit.
//!
//! The rendered tree contains only rendered files. Everything about *what
//! produced it* lives in the commit message, where it is attached to the tree
//! it describes, readable with plain Git, and absent from every diff.
//! See `docs/adr/008-provenance-in-trailers.md`.

use std::fmt;

use crate::data::Provenance as DataProvenance;
use crate::git::Oid;

/// Trailer keys. Named constants because they are parsed back, and a typo in
/// one half of a round trip is otherwise silent.
mod keys {
    pub const SOURCE: &str = "Template-Source";
    pub const REF: &str = "Template-Ref";
    pub const COMMIT: &str = "Template-Commit";
    pub const DIRTY: &str = "Template-Dirty";
    pub const ANSWERS: &str = "Answers-Digest";
    pub const DATA: &str = "Data-Source";
    pub const VERSION: &str = "Tpl-Version";
}

/// The `Template-Ref` value used when rendering an uncommitted working tree.
///
/// Not a revision anyone can resolve later, which is exactly what it is saying.
pub const WORKTREE_REF: &str = "<worktree>";

/// What produced a rendered commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// The template source, as configured.
    pub source: String,
    /// The revision that was asked for — a branch, tag or SHA.
    pub reference: String,
    /// The commit it resolved to.
    pub commit: Oid,
    /// Whether the template's uncommitted working tree was rendered.
    pub dirty: bool,
    /// A digest of the answers.
    pub answers_digest: String,
    /// The data sources that contributed, in load order.
    pub data: Vec<DataProvenance>,
    /// The git-tpl version that rendered it.
    pub version: String,
    /// The template's name, for the subject line.
    pub template_name: String,
}

impl Provenance {
    /// The full commit message: a subject, a blank line, then the trailers.
    pub fn to_message(&self) -> String {
        let mut message = String::new();

        // A subject a human can read in `git log --oneline`. The revision is
        // included because "which template version is this?" is the first
        // question anyone asks of the ref's history.
        message.push_str(&format!(
            "tpl: render {} at {}\n\n",
            self.template_name, self.reference
        ));

        message.push_str(&format!("{}: {}\n", keys::SOURCE, self.source));
        message.push_str(&format!("{}: {}\n", keys::REF, self.reference));
        message.push_str(&format!("{}: {}\n", keys::COMMIT, self.commit));
        if self.dirty {
            // Only written when true. An absent trailer means clean, so the
            // common case costs nothing and cannot be misread.
            message.push_str(&format!("{}: true\n", keys::DIRTY));
        }
        message.push_str(&format!("{}: {}\n", keys::ANSWERS, self.answers_digest));
        for data in &self.data {
            message.push_str(&format!(
                "{}: {} = {}\n",
                keys::DATA,
                data.name,
                data.trailer()
            ));
        }
        message.push_str(&format!("{}: {}\n", keys::VERSION, self.version));

        message
    }

    /// Read provenance back from a commit message.
    ///
    /// Returns `None` when the message carries no trailers we recognise, which
    /// is how a hand-made commit on the ref is tolerated rather than treated as
    /// corruption.
    pub fn parse(message: &str) -> Option<Recorded> {
        let mut recorded = Recorded::default();
        let mut found_any = false;

        for line in message.lines() {
            let Some((key, value)) = line.split_once(": ") else {
                continue;
            };
            let value = value.trim();

            match key.trim() {
                keys::SOURCE => {
                    recorded.source = Some(value.to_string());
                    found_any = true;
                }
                keys::REF => {
                    recorded.reference = Some(value.to_string());
                    found_any = true;
                }
                keys::COMMIT => {
                    recorded.commit = Oid::parse(value);
                    found_any = true;
                }
                keys::DIRTY => {
                    recorded.dirty = value.eq_ignore_ascii_case("true");
                    found_any = true;
                }
                keys::ANSWERS => {
                    recorded.answers_digest = Some(value.to_string());
                    found_any = true;
                }
                keys::DATA => {
                    if let Some((name, location)) = value.split_once(" = ") {
                        recorded
                            .data
                            .push((name.trim().to_string(), location.trim().to_string()));
                        found_any = true;
                    }
                }
                keys::VERSION => {
                    recorded.version = Some(value.to_string());
                    found_any = true;
                }
                _ => {}
            }
        }

        found_any.then_some(recorded)
    }
}

/// Provenance as read back from a commit.
///
/// Every field is optional because it comes from a commit message, which is
/// text an older git-tpl — or a person — may have written differently.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recorded {
    /// The template source.
    pub source: Option<String>,
    /// The revision that was asked for.
    pub reference: Option<String>,
    /// The commit it resolved to.
    pub commit: Option<Oid>,
    /// Whether an uncommitted working tree was rendered.
    pub dirty: bool,
    /// The digest of the answers.
    pub answers_digest: Option<String>,
    /// The data sources, as `(name, trailer)`.
    pub data: Vec<(String, String)>,
    /// The git-tpl version that rendered it.
    pub version: Option<String>,
}

impl Recorded {
    /// How to describe the revision in output.
    ///
    /// Delegates to [`crate::ops::describe_revision`] so that a revision read
    /// back from a commit and one just resolved are written identically —
    /// otherwise a `A → B` line would use two different formats either side of
    /// the arrow, which is exactly what it did before this was shared.
    pub fn describe_revision(&self) -> String {
        match (&self.reference, &self.commit) {
            (Some(reference), Some(commit)) => crate::ops::describe_revision(reference, *commit),
            (Some(reference), None) => reference.clone(),
            (None, Some(commit)) => commit.short(),
            (None, None) => "unknown".to_string(),
        }
    }
}

impl fmt::Display for Recorded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.describe_revision())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::SourceKind;

    fn oid(hex: &str) -> Oid {
        Oid::parse(hex).unwrap()
    }

    fn sample() -> Provenance {
        Provenance {
            source: "https://github.com/rawtools/rust-library".into(),
            reference: "v1.4.0".into(),
            commit: oid("4f2c1a9e6b3d8f05a1c7e2b94d6f8a03c5e17b29"),
            dirty: false,
            answers_digest:
                "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".into(),
            data: vec![DataProvenance {
                name: "licenses".into(),
                kind: SourceKind::TemplateFile,
                location: "data/licenses.toml".into(),
                revision: Some(oid("4f2c1a9e6b3d8f05a1c7e2b94d6f8a03c5e17b29")),
                checksum: None,
            }],
            version: "0.1.0".into(),
            template_name: "rust-library".into(),
        }
    }

    /// `status` reads these back to report the previous revision, so a break in
    /// either half of the round trip silently degrades every status report.
    #[test]
    fn provenance_round_trips_through_a_commit_message() {
        let original = sample();
        let recorded = Provenance::parse(&original.to_message()).unwrap();

        assert_eq!(recorded.source.as_deref(), Some(original.source.as_str()));
        assert_eq!(recorded.reference.as_deref(), Some("v1.4.0"));
        assert_eq!(recorded.commit, Some(original.commit));
        assert_eq!(
            recorded.answers_digest.as_deref(),
            Some(original.answers_digest.as_str())
        );
        assert_eq!(recorded.version.as_deref(), Some("0.1.0"));
        assert!(!recorded.dirty);
        assert_eq!(
            recorded.data,
            [(
                "licenses".to_string(),
                "template:data/licenses.toml@4f2c1a9".to_string()
            )]
        );
    }

    #[test]
    fn the_subject_names_the_template_and_the_revision() {
        let message = sample().to_message();
        let subject = message.lines().next().unwrap();
        assert_eq!(subject, "tpl: render rust-library at v1.4.0");
        assert_eq!(message.lines().nth(1), Some(""), "a blank line must follow");
    }

    /// An absent trailer means clean, so the common case costs nothing.
    #[test]
    fn the_dirty_trailer_is_written_only_when_it_is_true() {
        assert!(!sample().to_message().contains("Template-Dirty"));

        let dirty = Provenance {
            dirty: true,
            reference: WORKTREE_REF.into(),
            ..sample()
        };
        let recorded = Provenance::parse(&dirty.to_message()).unwrap();
        assert!(recorded.dirty);
    }

    #[test]
    fn several_data_sources_all_round_trip() {
        let provenance = Provenance {
            data: vec![
                DataProvenance {
                    name: "licenses".into(),
                    kind: SourceKind::TemplateFile,
                    location: "data/licenses.toml".into(),
                    revision: Some(oid("4f2c1a9e6b3d8f05a1c7e2b94d6f8a03c5e17b29")),
                    checksum: None,
                },
                DataProvenance {
                    name: "overrides".into(),
                    kind: SourceKind::LocalFile,
                    location: "config/tpl.toml".into(),
                    revision: None,
                    checksum: None,
                },
            ],
            ..sample()
        };

        let recorded = Provenance::parse(&provenance.to_message()).unwrap();
        assert_eq!(recorded.data.len(), 2);
        assert_eq!(recorded.data[1].1, "local:config/tpl.toml");
    }

    /// A hand-made commit on the ref is tolerated rather than treated as
    /// corruption — the ref is a normal ref and people may commit to it.
    #[test]
    fn a_message_with_no_trailers_yields_nothing() {
        assert_eq!(Provenance::parse("just a commit message\n"), None);
    }

    #[test]
    fn unrecognised_trailers_are_ignored() {
        let recorded = Provenance::parse(
            "tpl: render x at main\n\n\
             Template-Ref: main\n\
             Signed-off-by: Someone <s@example.com>\n\
             Co-authored-by: Other <o@example.com>\n",
        )
        .unwrap();

        assert_eq!(recorded.reference.as_deref(), Some("main"));
    }

    /// A branch name alone cannot tell you whether the template moved.
    #[test]
    fn a_revision_is_described_with_both_its_name_and_its_sha() {
        let recorded = Provenance::parse(&sample().to_message()).unwrap();
        assert_eq!(recorded.describe_revision(), "v1.4.0 (4f2c1a9)");
    }

    #[test]
    fn a_worktree_render_is_described_as_having_uncommitted_changes() {
        let dirty = Provenance {
            dirty: true,
            reference: WORKTREE_REF.into(),
            ..sample()
        };
        let recorded = Provenance::parse(&dirty.to_message()).unwrap();
        assert_eq!(
            recorded.describe_revision(),
            "4f2c1a9 (+ uncommitted changes)"
        );
    }
}
