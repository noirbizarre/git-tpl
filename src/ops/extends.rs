//! Merging an `[extends]` chain's manifests into one effective manifest.
//!
//! See `docs/adr/034-template-inheritance.md`. Fetching each ancestor and
//! walking the chain lives in `resolve.rs`, alongside `Resolved` and
//! `Ancestor`, which this module has no need to know about; this module is
//! the pure, Git-independent half — pinning, cycle/depth limits as data, and
//! folding a slice of already-resolved manifests into one.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use miette::Diagnostic;
use thiserror::Error;

use crate::git::GitBackend;
use crate::template::{Manifest, ManifestError};

/// A chain deeper than this — leaf plus ancestors — is rejected.
///
/// A constant, not part of the manifest format: raising it later costs
/// nothing, since no template author writes a number anywhere. Chosen
/// generously for a realistic organisation hierarchy (base -> language ->
/// team -> project) while still catching a chain that grew by accident.
pub const MAX_DEPTH: usize = 8;

/// Errors from resolving or merging an `[extends]` chain.
#[derive(Debug, Error, Diagnostic)]
pub enum ExtendsError {
    /// An ancestor's manifest is invalid.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Manifest(#[from] ManifestError),

    /// An ancestor's `rev` does not name a tag or a commit.
    #[error("`{origin}`'s `rev = \"{rev}\"` is not pinned")]
    #[diagnostic(
        code(tpl::extends::unpinned),
        help(
            "a parent must be pinned to a tag or a commit SHA, never a branch \
             — see docs/adr/034-template-inheritance.md"
        )
    )]
    Unpinned {
        /// The ancestor's own source.
        // Not named `source`: thiserror reserves that name for `#[source]`.
        origin: String,
        /// The `rev` that was not pinned.
        rev: String,
    },

    /// The chain revisits a template it has already resolved.
    #[error("cyclic `[extends]` chain: {}", path.join(" → "))]
    #[diagnostic(
        code(tpl::extends::cycle),
        help("a template may not, directly or indirectly, extend itself")
    )]
    Cycle {
        /// The chain, as a readable path, closed by repeating the first entry.
        path: Vec<String>,
    },

    /// The chain is deeper than [`MAX_DEPTH`].
    #[error("`[extends]` chain is deeper than {limit} layers")]
    #[diagnostic(
        code(tpl::extends::depth),
        help("split the chain, or flatten an intermediate layer into its own parent")
    )]
    Depth {
        /// The limit that was exceeded.
        limit: usize,
    },

    /// A `remove` path is not in the parent's own tree.
    #[error("`{origin}`: `remove` names `{path}`, which does not exist in the parent")]
    #[diagnostic(
        code(tpl::extends::remove_missing),
        help(
            "`remove` paths are relative to the parent's own repository root, \
             including its `root` prefix — see docs/adr/034-template-inheritance.md"
        )
    )]
    RemoveMissing {
        /// The child's own `[extends].source` — the one that declared `remove`.
        // Not named `source`: thiserror reserves that name for `#[source]`.
        origin: String,
        /// The path that does not exist in the parent.
        path: String,
    },
}

/// Whether `rev` is pinned: a tag or a commit SHA, never a branch (ADR-034).
///
/// A tag and a branch can be spelled identically, so this has to ask the
/// actual repository rather than guess from the string.
pub fn is_pinned(repo: &dyn GitBackend, rev: &str) -> Result<bool, crate::git::GitError> {
    Ok(looks_like_sha(rev) || repo.is_tag(rev)?)
}

/// A plausible abbreviated or full commit SHA.
///
/// Git accepts abbreviations down to 4 characters, but a 4-6 character hex
/// string is also a very ordinary word or a licence identifier fragment; 7 is
/// where Git's own `--short` output starts, and is used here as where treating
/// a string as "obviously a SHA" stops being a guess.
fn looks_like_sha(rev: &str) -> bool {
    (7..=40).contains(&rev.len()) && rev.chars().all(|c| c.is_ascii_hexdigit())
}

/// The merged result of folding an `[extends]` chain.
#[derive(Debug)]
pub struct Merged {
    /// The effective manifest: `name`/`description`/`root`/`strict`/`note`/
    /// `note_file` from the leaf; `questions`/`computed`/`data`/`remotes`
    /// folded root-to-leaf, by name.
    pub manifest: Manifest,
    /// `[data]` entry name -> index into the ancestor chain (`0` = the
    /// nearest parent) that declared the entry currently in effect. Absent
    /// for an entry the leaf declared or overrode.
    pub data_origin: BTreeMap<String, usize>,
    /// `[questions.<name>]` -> index into the ancestor chain (`0` = the
    /// nearest parent) that declared the question currently in effect.
    /// Absent for one the leaf declared or overrode.
    ///
    /// Read by `git tpl context --json` so a chain can be debugged without
    /// cloning every ancestor by hand to find which one wrote a given
    /// question. `[data]` needs the equivalent for a different reason too —
    /// reading a `kind = "template"` file from the right tree — which is why
    /// it existed first; this one exists purely for that debugging.
    pub question_origin: BTreeMap<String, usize>,
}

/// Merge a chain's manifests into one effective manifest.
///
/// `layers` is ordered nearest first: the leaf (the template actually
/// resolved), then its parent, then its grandparent, and so on — the same
/// order [`crate::ops::resolve::Resolved`] keeps its ancestors in. Folding
/// itself walks root-to-leaf, so an override keeps the position of its first
/// (furthest, root-most) declaration, and a name new to a nearer layer is
/// appended after it — `IndexMap::insert` already keeps an existing key's
/// position when only its value changes, which is exactly this rule with no
/// extra bookkeeping needed.
pub fn merge_chain(layers: &[&Manifest]) -> Result<Merged, ExtendsError> {
    let leaf = layers[0];

    let mut manifest = Manifest {
        name: leaf.name.clone(),
        description: leaf.description.clone(),
        root: leaf.root.clone(),
        strict: leaf.strict,
        data: BTreeMap::new(),
        questions: IndexMap::new(),
        computed: IndexMap::new(),
        note: leaf.note.clone(),
        note_file: leaf.note_file.clone(),
        remotes: IndexMap::new(),
        // Kept for introspection (`git tpl status`, `--json`) even though
        // nothing merges further from it -- the chain has already been
        // walked by the time this runs.
        extends: leaf.extends.clone(),
    };

    let mut data_origin: BTreeMap<String, usize> = BTreeMap::new();
    let mut question_origin: BTreeMap<String, usize> = BTreeMap::new();

    // Root-to-leaf: `layers` is nearest-first, so this is `.rev()`.
    for (layer_index, layer) in layers.iter().enumerate().rev() {
        merge_by_name(&mut manifest.questions, &layer.questions);
        merge_by_name(&mut manifest.computed, &layer.computed);
        merge_by_name(&mut manifest.remotes, &layer.remotes);

        for (name, decl) in &layer.data {
            manifest.data.insert(name.clone(), decl.clone());
            if layer_index == 0 {
                // The leaf's own declaration always wins outright; nothing
                // further out is read for it.
                data_origin.remove(name);
            } else {
                data_origin.insert(name.clone(), layer_index - 1);
            }
        }

        // Same rule, same reason, for questions: the last layer visited
        // (root-to-leaf, so the nearest one that declares a name) is the one
        // in effect, and re-declaring the same name at a nearer layer must
        // erase an origin recorded for a further one.
        for name in layer.questions.keys() {
            if layer_index == 0 {
                question_origin.remove(name);
            } else {
                question_origin.insert(name.clone(), layer_index - 1);
            }
        }
    }

    check_kind_collisions(&manifest)?;

    Ok(Merged {
        manifest,
        data_origin,
        question_origin,
    })
}

/// Fold one name-keyed declaration into another, by name.
///
/// `IndexMap::insert` keeps an existing key's position and only replaces its
/// value, which is already "the unit of override is the name, and an
/// override keeps its first declaration's position" (ADR-034) — there is
/// nothing else for this function to decide.
fn merge_by_name<V: Clone>(into: &mut IndexMap<String, V>, from: &IndexMap<String, V>) {
    for (name, value) in from {
        into.insert(name.clone(), value.clone());
    }
}

/// A name declared as a question by one layer and a computed value by
/// another. Each layer's own manifest already rejects this within itself
/// (`ManifestError::NameCollision`); this is the same ambiguity, found only
/// once the chain is merged.
fn check_kind_collisions(manifest: &Manifest) -> Result<(), ManifestError> {
    for name in manifest.computed.keys() {
        if manifest.questions.contains_key(name) {
            return Err(ManifestError::ExtendsKindCollision {
                name: name.clone(),
                this: "computed value".into(),
                other: "question".into(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::MANIFEST_NAME;
    use rstest::rstest;

    fn manifest(toml: &str) -> Manifest {
        Manifest::parse(toml, MANIFEST_NAME).expect("manifest should parse")
    }

    #[rstest]
    #[case("a1b2c3d", true)] // abbreviated SHA
    #[case("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2", true)] // full SHA
    #[case("v1.0.0", false)]
    #[case("main", false)]
    #[case("abc", false)] // too short to be treated as a SHA
    fn sha_shaped_revs_are_recognised(#[case] rev: &str, #[case] expected: bool) {
        assert_eq!(looks_like_sha(rev), expected);
    }

    #[test]
    fn a_chain_of_one_is_the_identity() {
        let leaf = manifest(
            r#"
            name = "child"
            [questions.a]
            type = "string"
            "#,
        );
        let merged = merge_chain(&[&leaf]).unwrap();
        assert_eq!(merged.manifest.name, "child");
        assert_eq!(merged.manifest.questions.keys().collect::<Vec<_>>(), ["a"]);
        assert!(merged.data_origin.is_empty());
    }

    /// Ancestors' questions first, in ancestor order, then the leaf's new
    /// ones -- the order stated in ADR-034.
    #[test]
    fn ancestor_questions_come_before_the_new_ones_the_leaf_adds() {
        let leaf = manifest(
            r#"
            name = "child"
            [questions.own]
            type = "string"
            "#,
        );
        let parent = manifest(
            r#"
            name = "parent"
            [questions.first]
            type = "string"
            [questions.second]
            type = "string"
            "#,
        );
        let merged = merge_chain(&[&leaf, &parent]).unwrap();
        assert_eq!(
            merged.manifest.questions.keys().collect::<Vec<_>>(),
            ["first", "second", "own"]
        );
    }

    /// An override keeps the position of its first (furthest) declaration —
    /// inserting an override must not reshuffle the prompt sequence.
    #[test]
    fn an_overridden_question_keeps_the_parents_position() {
        let leaf = manifest(
            r#"
            name = "child"
            [questions.second]
            type = "string"
            prompt = "overridden"
            "#,
        );
        let parent = manifest(
            r#"
            name = "parent"
            [questions.first]
            type = "string"
            [questions.second]
            type = "string"
            prompt = "original"
            [questions.third]
            type = "string"
            "#,
        );
        let merged = merge_chain(&[&leaf, &parent]).unwrap();
        assert_eq!(
            merged.manifest.questions.keys().collect::<Vec<_>>(),
            ["first", "second", "third"]
        );
        assert_eq!(
            merged.manifest.questions["second"].prompt.as_deref(),
            Some("overridden")
        );
    }

    #[test]
    fn name_description_and_root_come_from_the_leaf_alone() {
        let leaf = manifest(
            r#"
            name = "child"
            description = "the child"
            "#,
        );
        let parent = manifest(
            r#"
            name = "parent"
            description = "the parent"
            root = "src"
            "#,
        );
        let merged = merge_chain(&[&leaf, &parent]).unwrap();
        assert_eq!(merged.manifest.name, "child");
        assert_eq!(merged.manifest.root, "template");
    }

    /// `[data]` merges by name like everything else, and an entry an
    /// ancestor contributes (and the leaf does not override) is tagged with
    /// which ancestor declared it, so it can be read from that layer's own
    /// tree later.
    #[test]
    fn data_sources_merge_by_name_and_record_their_origin() {
        let leaf = manifest(
            r#"
            name = "child"
            [data.own]
            source = "data/own.toml"
            "#,
        );
        let parent = manifest(
            r#"
            name = "parent"
            [data.shared]
            source = "data/shared.toml"
            "#,
        );
        let merged = merge_chain(&[&leaf, &parent]).unwrap();
        assert_eq!(merged.manifest.data.len(), 2);
        assert_eq!(merged.data_origin.get("shared"), Some(&0));
        assert_eq!(merged.data_origin.get("own"), None);
    }

    /// `[questions]` records its origin the same way `[data]` does, for
    /// `git tpl context --json` to report which layer a question came from.
    #[test]
    fn questions_record_their_origin_the_same_way_data_does() {
        let leaf = manifest(
            r#"
            name = "child"
            [questions.own]
            type = "string"
            "#,
        );
        let parent = manifest(
            r#"
            name = "parent"
            [questions.inherited]
            type = "string"
            "#,
        );
        let merged = merge_chain(&[&leaf, &parent]).unwrap();
        assert_eq!(merged.question_origin.get("inherited"), Some(&0));
        assert_eq!(merged.question_origin.get("own"), None);
    }

    /// A leaf overriding an ancestor's question owns it outright, the same
    /// way overriding a data source does.
    #[test]
    fn a_leaf_overriding_a_question_owns_it_outright() {
        let leaf = manifest(
            r#"
            name = "child"
            [questions.shared]
            type = "string"
            prompt = "overridden"
            "#,
        );
        let parent = manifest(
            r#"
            name = "parent"
            [questions.shared]
            type = "string"
            prompt = "original"
            "#,
        );
        let merged = merge_chain(&[&leaf, &parent]).unwrap();
        assert_eq!(merged.question_origin.get("shared"), None);
    }

    /// A leaf overriding an ancestor's data source reads from the leaf's own
    /// tree, not the ancestor's.
    #[test]
    fn a_leaf_overriding_a_data_source_owns_it_outright() {
        let leaf = manifest(
            r#"
            name = "child"
            [data.shared]
            source = "data/child-version.toml"
            "#,
        );
        let parent = manifest(
            r#"
            name = "parent"
            [data.shared]
            source = "data/parent-version.toml"
            "#,
        );
        let merged = merge_chain(&[&leaf, &parent]).unwrap();
        assert_eq!(
            merged.manifest.data["shared"].source,
            "data/child-version.toml"
        );
        assert_eq!(merged.data_origin.get("shared"), None);
    }

    /// The same ambiguity a same-manifest name collision catches, found only
    /// once the chain is merged.
    #[test]
    fn a_cross_layer_kind_collision_is_rejected() {
        let leaf = manifest(
            r#"
            name = "child"
            [computed]
            shared = "{{ 1 }}"
            "#,
        );
        let parent = manifest(
            r#"
            name = "parent"
            [questions.shared]
            type = "string"
            "#,
        );
        let error = merge_chain(&[&leaf, &parent]).unwrap_err();
        std::assert_matches!(
            error,
            ExtendsError::Manifest(ManifestError::ExtendsKindCollision { ref name, .. }) if name == "shared"
        );
    }
}
