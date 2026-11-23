//! Provenance, recorded as commit trailers on the rendered commit.
//!
//! The rendered tree contains only rendered files. Everything about *what
//! produced it* lives in the commit message, where it is attached to the tree
//! it describes, readable with plain Git, and absent from every diff.
//! See `docs/adr/008-provenance-in-trailers.md`.

use std::fmt;

use crate::data::Provenance as DataProvenance;
use crate::git::Oid;

/// Data evidence belongs to a module instance, not just its shared child revision.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ComposedDataProvenance {
    /// Parent question identifying the composition declaration.
    pub from: String,
    /// Selected module ID within that declaration.
    pub module: String,
    /// The child's declared source name, independent of other child namespaces.
    pub name: String,
    /// Existing data trailer value, including resolved revision or checksum.
    pub location: String,
}

/// The exact selected child revision used to render one group of modules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedRevision {
    /// Unique question name supplying this child's module IDs.
    pub from: String,
    /// Evaluated child repository address.
    pub source: String,
    /// The reference requested, which may be a moving branch.
    pub reference: String,
    /// The commit actually rendered.
    pub revision: Oid,
    /// Historical child ancestry; `None` distinguishes legacy records from a known empty chain.
    pub extends: Option<Vec<ExtendsProvenance>>,
}

impl ComposedRevision {
    /// Adding ancestry evidence alone must not manufacture an update commit for legacy history.
    pub fn matches_recorded(&self, recorded: &Self) -> bool {
        self.lock() == recorded.lock()
            && recorded
                .extends
                .as_ref()
                .is_none_or(|chain| self.extends.as_ref() == Some(chain))
    }

    /// The editable project lock for this child.
    pub fn lock(&self) -> crate::config::ComposedRef {
        crate::config::ComposedRef {
            from: self.from.clone(),
            source: self.source.clone(),
            reference: self.reference.clone(),
            commit: self.revision.to_hex(),
        }
    }
}

/// Trailer keys. Named constants because they are parsed back, and a typo in
/// one half of a round trip is otherwise silent.
mod keys {
    pub const SOURCE: &str = "Template-Source";
    pub const REF: &str = "Template-Ref";
    pub const COMMIT: &str = "Template-Commit";
    pub const DIRTY: &str = "Template-Dirty";
    pub const ANSWERS: &str = "Answers-Digest";
    pub const DATA: &str = "Data-Source";
    pub const EXTENDS: &str = "Template-Extends";
    pub const COMPOSE: &str = "Template-Compose";
    pub const COMPOSE_DATA: &str = "Template-Compose-Data";
    pub const ONCE: &str = "Template-Once";
    pub const VERSION: &str = "Tpl-Version";
}

/// The `Template-Ref` value used when rendering an uncommitted working tree.
///
/// Not a revision anyone can resolve later, which is exactly what it is saying.
pub const WORKTREE_REF: &str = crate::revision::WORKTREE_REF;

/// What produced a rendered commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// The template source, as configured.
    pub source: String,
    /// The reference that was asked for — a branch, tag or SHA.
    pub reference: String,
    /// The commit it resolved to.
    pub revision: Oid,
    /// Whether the template's uncommitted working tree was rendered.
    pub dirty: bool,
    /// A digest of the answers.
    pub answers_digest: String,
    /// The data sources that contributed, in load order.
    pub data: Vec<DataProvenance>,
    /// The ancestor chain, nearest parent first, root ancestor last. Empty for
    /// a template with no `[extends]`.
    pub extends: Vec<ExtendsProvenance>,
    /// Exact selected child commits, even when a moving branch renders identical files.
    pub composed: Vec<ComposedRevision>,
    /// Actually loaded child data, in declaration, module and load order.
    pub composed_data: Vec<ComposedDataProvenance>,
    /// Permanently captured output paths; their bytes and modes remain in the tree.
    pub once: std::collections::BTreeSet<String>,
    /// The git-tpl version that rendered it.
    pub version: String,
    /// The template's name, for the subject line.
    pub template_name: String,
}

/// One ancestor in an `[extends]` chain, as recorded in the trailers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtendsProvenance {
    /// The ancestor's own `[extends].source`.
    pub source: String,
    /// The commit it resolved to.
    pub revision: Oid,
}

impl ExtendsProvenance {
    /// `<source>@<commit>` — positional rather than named like
    /// `Data-Source`: an ancestor has no name a template author chose, and its
    /// position in the chain is its identity.
    fn trailer(&self) -> String {
        format!("{}@{}", self.source, self.revision)
    }
}

impl Provenance {
    /// Preview and publication must agree about policy changes even when output bytes are identical.
    pub fn matches_recorded_state(&self, recorded: &Recorded) -> bool {
        self.once == recorded.once
            && self.composed.len() == recorded.composed.len()
            && self
                .composed
                .iter()
                .zip(&recorded.composed)
                .all(|(new, old)| new.matches_recorded(old))
    }

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
        message.push_str(&format!("{}: {}\n", keys::COMMIT, self.revision));
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
        for extends in &self.extends {
            message.push_str(&format!("{}: {}\n", keys::EXTENDS, extends.trailer()));
        }
        for child in &self.composed {
            let extends = child.extends.as_ref().map(|chain| {
                chain
                    .iter()
                    .map(|ancestor| {
                        serde_json::json!({
                            "source": ancestor.source, "commit": ancestor.revision.to_hex(),
                        })
                    })
                    .collect::<Vec<_>>()
            });
            // JSON escapes URL/ref separators and control characters, so this trailer round-trips even for scp URLs.
            message.push_str(&format!(
                "{}: {}\n",
                keys::COMPOSE,
                serde_json::json!({
                    "from": child.from, "source": child.source, "ref": child.reference,
                    "commit": child.revision.to_hex(),
                    "extends": extends,
                    "ancestryVersion": 1,
                })
            ));
        }
        for data in &self.composed_data {
            // Structured scope prevents collisions and escapes control characters in data locations.
            message.push_str(&format!(
                "{}: {}\n",
                keys::COMPOSE_DATA,
                serde_json::to_string(data).expect("string-only provenance is serializable")
            ));
        }
        for path in &self.once {
            // JSON keeps path control characters on a single trailer line.
            message.push_str(&format!("{}: {}\n", keys::ONCE, serde_json::json!(path)));
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
                    recorded.revision = Oid::parse(value);
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
                keys::EXTENDS => {
                    // Split from the *right*: an scp-style Git URL
                    // (`git@host:path`) can itself contain an `@`, but the
                    // trailing `@<short-sha>` never does, so `rsplit_once` is
                    // the one split that is always unambiguous here — the
                    // same reasoning `Data-Source`'s `name = value` avoids by
                    // using a different separator, applied to a value that
                    // cannot change its own separator.
                    if let Some((source, revision)) = value.rsplit_once('@') {
                        recorded
                            .extends
                            .push((source.trim().to_string(), revision.trim().to_string()));
                        found_any = true;
                    }
                }
                keys::COMPOSE => {
                    let Ok(value) = serde_json::from_str::<serde_json::Value>(value) else {
                        continue;
                    };
                    let (Some(from), Some(source), Some(reference), Some(revision)) = (
                        value.get("from").and_then(|v| v.as_str()),
                        value.get("source").and_then(|v| v.as_str()),
                        value.get("ref").and_then(|v| v.as_str()),
                        value
                            .get("commit")
                            .and_then(|v| v.as_str())
                            .and_then(Oid::parse),
                    ) else {
                        continue;
                    };
                    recorded.composed.push(ComposedRevision {
                        from: from.to_string(),
                        source: source.to_string(),
                        reference: reference.to_string(),
                        revision,
                        extends: value.get("extends").and_then(|chain| {
                            chain
                                .as_array()?
                                .iter()
                                .map(|ancestor| {
                                    Some(ExtendsProvenance {
                                        source: ancestor.get("source")?.as_str()?.to_string(),
                                        revision: Oid::parse(ancestor.get("commit")?.as_str()?)?,
                                    })
                                })
                                .collect()
                        }),
                    });
                    found_any = true;
                }
                keys::COMPOSE_DATA => {
                    // A malformed individual trailer must not hide valid siblings or legacy provenance.
                    if let Ok(data) = serde_json::from_str::<ComposedDataProvenance>(value) {
                        recorded.composed_data.push(data);
                        found_any = true;
                    }
                }
                keys::ONCE => {
                    if let Ok(path) = serde_json::from_str::<String>(value) {
                        recorded.once.insert(path);
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
    pub revision: Option<Oid>,
    /// Whether an uncommitted working tree was rendered.
    pub dirty: bool,
    /// The digest of the answers.
    pub answers_digest: Option<String>,
    /// The data sources, as `(name, trailer)`.
    pub data: Vec<(String, String)>,
    /// The ancestor chain, as `(source, short-sha)` pairs, nearest parent
    /// first — the same shape `data` already takes for the same reason: this
    /// is text a person or an older git-tpl may have written, not a value
    /// this crate re-resolved.
    pub extends: Vec<(String, String)>,
    /// Child commits of that historical rendered ref, not moving branch tips.
    pub composed: Vec<ComposedRevision>,
    /// Scoped child data evidence; absent in older renderings, not proof of unused data.
    pub composed_data: Vec<ComposedDataProvenance>,
    /// Cumulative captured paths, empty for renderings predating one-shot files.
    pub once: std::collections::BTreeSet<String>,
    /// The git-tpl version that rendered it.
    pub version: Option<String>,
}

impl Recorded {
    /// How to describe the revision in output.
    ///
    /// Delegates to the shared revision formatter so that a revision read
    /// back from a commit and one just resolved are written identically —
    /// otherwise a `A → B` line would use two different formats either side of
    /// the arrow, which is exactly what it did before this was shared.
    pub fn describe_revision(&self) -> String {
        match (&self.reference, &self.revision) {
            (Some(reference), Some(revision)) => {
                crate::revision::describe_revision(reference, *revision)
            }
            (Some(reference), None) => reference.clone(),
            (None, Some(revision)) => revision.short(),
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

    #[test]
    fn state_comparison_tolerates_legacy_ancestry_but_detects_recorded_changes() {
        let mut provenance = sample();
        provenance.composed.push(ComposedRevision {
            from: "modules".into(),
            source: "child".into(),
            reference: "main".into(),
            revision: provenance.revision,
            extends: Some(Vec::new()),
        });
        let mut recorded = Provenance::parse(&provenance.to_message()).unwrap();
        assert!(provenance.matches_recorded_state(&recorded));
        recorded.composed[0].extends = None;
        assert!(provenance.matches_recorded_state(&recorded));
        recorded.composed[0].extends = Some(vec![ExtendsProvenance {
            source: "ancestor".into(),
            revision: provenance.revision,
        }]);
        assert!(!provenance.matches_recorded_state(&recorded));
        recorded.composed[0].extends = None;
        recorded.composed[0].reference = "other".into();
        assert!(!provenance.matches_recorded_state(&recorded));
    }

    #[test]
    fn scoped_child_data_round_trips_without_hiding_legacy_or_valid_sibling_evidence() {
        let mut provenance = sample();
        for module in ["one", "two\nTemplate-Source: injected"] {
            provenance.composed_data.push(ComposedDataProvenance {
                from: "sources".into(),
                module: module.into(),
                name: "catalog".into(),
                location: "git:git@host:repo:data/\"quoted\".toml@abcdef0".into(),
            });
        }
        let message = format!(
            "{}Template-Compose-Data: {{broken}}\nTemplate-Compose-Data: {{\"from\":\"incomplete\"}}\n",
            provenance.to_message()
        );
        let parsed = Provenance::parse(&message).unwrap();
        assert_eq!(parsed.composed_data, provenance.composed_data);
        assert_eq!(parsed.source, Some(provenance.source));
        assert_eq!(
            parsed.data,
            Provenance::parse(&sample().to_message()).unwrap().data
        );
        assert!(
            Provenance::parse(&sample().to_message())
                .unwrap()
                .composed_data
                .is_empty()
        );
        let extended = "Template-Compose-Data: {\"from\":\"sources\",\"module\":\"one\",\"name\":\"catalog\",\"location\":\"local:data.toml\",\"future\":true}";
        assert_eq!(Provenance::parse(extended).unwrap().composed_data.len(), 1);
    }

    #[test]
    fn captured_paths_round_trip_in_sorted_order_without_injecting_trailers() {
        let mut provenance = sample();
        provenance.once = ["z\nTemplate-Source: injected", "a/\"quoted\""]
            .into_iter()
            .map(String::from)
            .collect();
        let message = provenance.to_message();
        assert_eq!(Provenance::parse(&message).unwrap().once, provenance.once);
        assert!(
            message.find("Template-Once: \"a/").unwrap()
                < message.find("Template-Once: \"z").unwrap()
        );
        let message = format!(
            "{message}Template-Once: not-json\nTemplate-Once: 1\nTemplate-Once: \"a/\\\"quoted\\\"\"\n"
        );
        assert_eq!(Provenance::parse(&message).unwrap().once, provenance.once);
        assert!(
            Provenance::parse(&sample().to_message())
                .unwrap()
                .once
                .is_empty()
        );
    }

    fn oid(hex: &str) -> Oid {
        Oid::parse(hex).unwrap()
    }

    fn sample() -> Provenance {
        Provenance {
            source: "https://github.com/noirbizarre/rust-library".into(),
            reference: "v1.4.0".into(),
            revision: oid("4f2c1a9e6b3d8f05a1c7e2b94d6f8a03c5e17b29"),
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
            extends: Vec::new(),
            composed: Vec::new(),
            composed_data: Vec::new(),
            once: Default::default(),
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
        assert_eq!(recorded.revision, Some(original.revision));
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
    fn child_branch_and_exact_commit_survive_the_rendered_commit_trailers() {
        let child = ComposedRevision {
            from: "sources".into(),
            source: "git@example.com:org/source.tpl".into(),
            reference: "main".into(),
            revision: oid("1122334455667788990011223344556677889900"),
            extends: None,
        };
        let rendering = Provenance {
            composed: vec![child.clone()],
            ..sample()
        };
        let recorded = Provenance::parse(&rendering.to_message()).unwrap();
        assert_eq!(recorded.composed, vec![child]);
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

    /// `git tpl status` needs the whole chain back, nearest parent first, or a
    /// project rendered through inheritance cannot say what it came from.
    #[test]
    fn several_ancestors_all_round_trip() {
        let provenance = Provenance {
            extends: vec![
                ExtendsProvenance {
                    source: "https://github.com/org/parent".into(),
                    revision: oid("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"),
                },
                ExtendsProvenance {
                    source: "https://github.com/org/grandparent".into(),
                    revision: oid("1122334455667788990011223344556677889900"),
                },
            ],
            ..sample()
        };

        let message = provenance.to_message();
        let recorded = Provenance::parse(&message).unwrap();

        assert_eq!(
            recorded.extends,
            [
                (
                    "https://github.com/org/parent".to_string(),
                    "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string()
                ),
                (
                    "https://github.com/org/grandparent".to_string(),
                    "1122334455667788990011223344556677889900".to_string()
                ),
            ]
        );
        // Nearest parent first, root ancestor last -- the same order the
        // chain was resolved in, so the trailers read top-down like the chain
        // itself.
        let parent_line = message
            .lines()
            .position(|l| l.contains("org/parent"))
            .unwrap();
        let grandparent_line = message
            .lines()
            .position(|l| l.contains("org/grandparent"))
            .unwrap();
        assert!(parent_line < grandparent_line);
    }

    /// A template with no `[extends]` writes no trailer at all, so an
    /// ancestor-free rendering's commit message is unchanged by this feature.
    #[test]
    fn no_extends_trailer_is_written_for_a_template_with_no_parent() {
        assert!(!sample().to_message().contains("Template-Extends"));
    }

    /// An scp-style Git URL contains its own `@`, so the source and the
    /// trailing `@<short-sha>` must be split from the right, not the left.
    #[test]
    fn an_scp_style_ancestor_source_round_trips() {
        let provenance = Provenance {
            extends: vec![ExtendsProvenance {
                source: "git@github.com:org/parent.git".into(),
                revision: oid("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"),
            }],
            ..sample()
        };

        let recorded = Provenance::parse(&provenance.to_message()).unwrap();
        assert_eq!(
            recorded.extends,
            [(
                "git@github.com:org/parent.git".to_string(),
                "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string()
            )]
        );
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
