//! Retained inputs for proving backports without reopening mutable template references.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::context::Context;
use crate::eval::{Partials, Undefined};
use crate::git::Oid;
use crate::render::Rendered;

use super::{OpError, resolve::Resolved};

#[derive(Clone)]
pub(super) struct Environment {
    pub context: Context,
    pub partials: Arc<Partials>,
    pub undefined: Undefined,
}

#[derive(Clone)]
pub(super) struct Source {
    pub owner: String,
    pub revision: Oid,
    pub reference: String,
    pub path: String,
    pub bytes: Vec<u8>,
    pub environment: Arc<Environment>,
}

pub(super) type Trace = BTreeMap<String, Source>;

pub(super) fn capture(
    template: &Resolved,
    source: &str,
    files: &[Rendered],
    environment: Environment,
) -> Result<Trace, OpError> {
    let environment = Arc::new(environment);
    files
        .iter()
        // One-shot entries retain old output bytes, not a current template source to patch.
        .filter(|file| file.origin != crate::render::Origin::Retained)
        .map(|file| {
            let (owner, revision, reference, path, bytes) =
                template.backport_source(source, file)?;
            let origin = Source {
                owner,
                revision,
                reference,
                path,
                bytes,
                environment: environment.clone(),
            };
            Ok((file.path.clone(), origin))
        })
        .collect()
}
