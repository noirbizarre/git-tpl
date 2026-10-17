//! Deterministic sibling-template rendering, with separately pinned repositories and answer namespaces.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use miette::Diagnostic;
use thiserror::Error;

use super::{Answering, OpError, Request, Trust, render_resolved, resolve};
use crate::context::Context;
use crate::data::{Decision, REMOTE_LIMIT_BYTES, RemoteRequest, SourceKind};
use crate::eval::{Partials, Undefined, render_string_with};
use crate::provenance::ComposedRevision;
use crate::render::Rendered;
use crate::template::{ComposeDecl, Value};
use crate::userconfig::UserConfig;

/// Fail before writing a Git tree when a child cannot be pinned, rendered or placed safely.
#[derive(Debug, Error, Diagnostic)]
pub enum ComposeError {
    /// An answered module list contains an unsafe or repeated ID within that list.
    #[error("`[[compose]]` for `{from}`: {reason}")]
    #[diagnostic(
        code(tpl::compose::invalid),
        help("use unique lowercase hyphenated module IDs in a list answer")
    )]
    Invalid {
        /// Parent question naming the module list.
        from: String,
        /// Why its value cannot be used.
        reason: String,
    },

    /// A remote child was named by a template but never authorised by this caller.
    #[error("composed child `{origin}` is not trusted")]
    #[diagnostic(
        code(tpl::compose::untrusted),
        help("approve this child explicitly with --trust or a matching [trust] entry")
    )]
    Untrusted {
        /// Child repository refused before cloning.
        origin: String,
    },

    /// Two parents/children produced the same output name.
    #[error("a composed template would replace `{path}`")]
    #[diagnostic(
        code(tpl::compose::collision),
        help("give each module a unique path; composition never overrides another file")
    )]
    Collision {
        /// Duplicate rendered output path.
        path: String,
    },

    /// The child itself could not be resolved or rendered.
    #[error("could not render composed template `{origin}`")]
    #[diagnostic(code(tpl::compose::child_failed))]
    Child {
        /// Child repository that failed.
        origin: String,
        /// Original typed diagnostic.
        #[source]
        source: Box<OpError>,
    },

    /// The composed template contains another composition.
    #[error("composed template `{origin}` itself composes other templates")]
    #[diagnostic(
        code(tpl::compose::nested),
        help("put all sibling declarations in the parent to keep the revision graph bounded")
    )]
    Nested {
        /// Child repository with a recursive declaration.
        origin: String,
    },
}

fn valid_id(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 48
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

/// Render each child's ordered module list, allowing different lists to share an ID.
pub fn render(
    entries: &[ComposeDecl],
    context: &Context,
    partials: &Arc<Partials>,
    user: &UserConfig,
    trust: &mut Trust<'_>,
    parent: &mut Vec<Rendered>,
    recorded: Option<&[ComposedRevision]>,
) -> Result<Vec<ComposedRevision>, ComposeError> {
    let mut paths: HashSet<String> = parent.iter().map(|file| file.path.clone()).collect();
    let mut revisions = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let source = render_string_with(
            &entry.source,
            context,
            "compose.source",
            partials,
            Undefined::Strict,
        )
        .map_err(|error| ComposeError::Child {
            origin: entry.source.clone(),
            source: Box::new(error.into()),
        })?;
        let Some(Value::Array(items)) = context.answers().get(&entry.from) else {
            return Err(ComposeError::Invalid {
                from: entry.from.clone(),
                reason: "the answer must be a list of strings".into(),
            });
        };
        // Empty means an initially empty plugin; no child repository is cloned, fetched or read.
        if items.len() > 32 {
            return Err(ComposeError::Invalid {
                from: entry.from.clone(),
                reason: "more than 32 modules; split the project or choose fewer".into(),
            });
        }
        // Different child types can share a directory; only repeats within one list are ambiguous.
        let mut module_ids = HashSet::new();
        let mut ids = Vec::new();
        for item in items {
            let Some(id) = item.as_str() else {
                return Err(ComposeError::Invalid {
                    from: entry.from.clone(),
                    reason: "every module ID must be a string".into(),
                });
            };
            if !valid_id(id) || !module_ids.insert(id.to_string()) {
                return Err(ComposeError::Invalid {
                    from: entry.from.clone(),
                    reason: format!("`{id}` is invalid or duplicated"),
                });
            }
            ids.push(id);
        }
        if ids.is_empty() {
            continue;
        }
        let locked =
            recorded.and_then(|children| children.iter().find(|child| child.from == entry.from));
        if let Some(previous) = locked
            && (previous.source != source || previous.reference != entry.reference)
        {
            return Err(ComposeError::Invalid {
                from: entry.from.clone(),
                reason: "the recorded child source or ref differs from this rendering".into(),
            });
        }
        // Consent to a remote child is separate from trusting the parent. The resolved SHA is kept after fetching.
        if resolve::local_path(&source).is_none() && !user.trust.allows(&source) {
            let name = format!("compose:{index}");
            let requests = [RemoteRequest {
                name: name.clone(),
                source: source.clone(),
                kind: SourceKind::Git,
            }];
            let decisions = trust
                .gate()
                .confirm(&requests, REMOTE_LIMIT_BYTES)
                .map_err(|_| ComposeError::Untrusted {
                    origin: source.clone(),
                })?;
            if decisions.get(&name) != Some(&Decision::Allow) {
                return Err(ComposeError::Untrusted { origin: source });
            }
        }
        let pin = locked.map(|child| child.revision.to_hex());
        let child = resolve::resolve(
            Request {
                source: &source,
                reference: pin.as_deref().or(Some(&entry.reference)),
                root: None,
                dirty: false,
            },
            user,
            trust,
        )
        .map_err(|error| ComposeError::Child {
            origin: source.clone(),
            source: Box::new(error.into()),
        })?;
        if !child.manifest.compose.is_empty() {
            return Err(ComposeError::Nested {
                origin: source.clone(),
            });
        }
        revisions.push(ComposedRevision {
            from: entry.from.clone(),
            source: source.clone(),
            reference: entry.reference.clone(),
            revision: child.revision,
        });
        for id in ids {
            let mut answers = BTreeMap::new();
            answers.insert("module_name".into(), Value::String(id.to_string()));
            answers.insert("module_path".into(), Value::String(format!("modules/{id}")));
            for key in ["version", "memcastle"] {
                if child.manifest.questions.contains_key(key)
                    && let Some(value) = context.answers().get(key)
                {
                    answers.insert(key.to_string(), value.clone());
                }
            }
            let child_trust = match &mut *trust {
                Trust::Ask(gate) => Trust::Ask(*gate),
                Trust::Always(_) => Trust::always(),
                Trust::Refuse(_) => Trust::refuse(),
                Trust::Decided(decided) => Trust::Ask(decided),
            };
            let rendered = render_resolved(
                &child,
                &source,
                None,
                answers,
                user,
                Answering::defaults(),
                child_trust,
            )
            .map_err(|error| ComposeError::Child {
                origin: source.clone(),
                source: Box::new(error),
            })?;
            for file in rendered.files {
                if !paths.insert(file.path.clone()) {
                    return Err(ComposeError::Collision { path: file.path });
                }
                parent.push(file);
            }
        }
    }
    parent.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(revisions)
}
