//! Deterministic sibling-template rendering, with separately pinned repositories and answer namespaces.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use miette::Diagnostic;
use thiserror::Error;

use super::{Answering, OpError, Request, Trust, render_resolved, resolve};
use crate::context::Context;
use crate::data::{Decision, REMOTE_LIMIT_BYTES, RemoteRequest, SourceKind};
use crate::eval::{Partials, Undefined, render_string_with};
use crate::render::Rendered;
use crate::template::{ComposeDecl, Value};
use crate::userconfig::UserConfig;

/// Fail before writing a Git tree when a child cannot be pinned, rendered or placed safely.
#[derive(Debug, Error, Diagnostic)]
pub enum ComposeError {
    /// An answered module list contains an unsafe or repeated ID.
    #[error("`[[compose]]` for `{from}`: {reason}")]
    #[diagnostic(
        code(tpl::compose::invalid),
        help("use unique lowercase hyphenated module IDs, separated by commas")
    )]
    Invalid {
        /// Parent question naming the module list.
        from: String,
        /// Why its value cannot be used.
        reason: String,
    },

    /// The child repository revision can move.
    #[error("`[[compose]]` child `{origin}` is not pinned at `{reference}`")]
    #[diagnostic(
        code(tpl::compose::unpinned),
        help("pin each composed template to a tag or commit SHA")
    )]
    Unpinned {
        /// Child repository.
        origin: String,
        /// Reference requested by the parent.
        reference: String,
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

/// Render independent child repositories once per comma-separated module ID in the parent's recorded answer.
pub fn render(
    entries: &[ComposeDecl],
    context: &Context,
    partials: &Arc<Partials>,
    user: &UserConfig,
    trust: &mut Trust<'_>,
    parent: &mut Vec<Rendered>,
) -> Result<(), ComposeError> {
    let mut paths: HashSet<String> = parent.iter().map(|file| file.path.clone()).collect();
    let mut module_ids = HashSet::new();
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
        let Some(Value::String(list)) = context.answers().get(&entry.from) else {
            return Err(ComposeError::Invalid {
                from: entry.from.clone(),
                reason: "the answer must be a string".into(),
            });
        };
        // Empty means an initially empty plugin; no child repository is cloned, fetched or read.
        let ids: Vec<_> = if list.trim().is_empty() {
            Vec::new()
        } else {
            list.split(',').map(str::trim).collect()
        };
        if ids.len() > 32 {
            return Err(ComposeError::Invalid {
                from: entry.from.clone(),
                reason: "more than 32 modules; split the project or choose fewer".into(),
            });
        }
        for id in &ids {
            if !valid_id(id) || !module_ids.insert((*id).to_string()) {
                return Err(ComposeError::Invalid {
                    from: entry.from.clone(),
                    reason: format!("`{id}` is invalid or duplicated"),
                });
            }
        }
        if ids.is_empty() {
            continue;
        }
        // Consent to a remote child is separate from trusting the parent. A tag/commit is checked *after* fetching.
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
        let child = resolve::resolve(
            Request {
                source: &source,
                reference: Some(&entry.rev),
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
        let pinned =
            super::extends::is_pinned(child.repo.as_ref(), &entry.rev).map_err(|error| {
                ComposeError::Child {
                    origin: source.clone(),
                    source: Box::new(error.into()),
                }
            })?;
        if !pinned {
            return Err(ComposeError::Unpinned {
                origin: source.clone(),
                reference: entry.rev.clone(),
            });
        }
        if !child.manifest.compose.is_empty() {
            return Err(ComposeError::Nested {
                origin: source.clone(),
            });
        }
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
    Ok(())
}
