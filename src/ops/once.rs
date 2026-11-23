//! Historical output is carried by the rendered tree, never reconstructed from a user's worktree.

use std::collections::BTreeMap;

use super::{OpError, Provenance};
use crate::git::{GitBackend, Oid};
use crate::render::{Origin, Rendered};

pub(super) type Baseline = BTreeMap<String, Rendered>;

pub(super) fn for_project(
    project: Option<(&dyn GitBackend, &std::path::Path)>,
) -> Result<Baseline, OpError> {
    let Some((repo, root)) = project.filter(|(_, root)| crate::config::Config::exists_in(root))
    else {
        return Ok(Baseline::new());
    };
    let (_, reference) = super::identify(root)?;
    load(repo, repo.resolve_ref(&reference)?)
}

pub(super) fn load(project: &dyn GitBackend, tip: Option<Oid>) -> Result<Baseline, OpError> {
    let Some(tip) = tip else {
        return Ok(Baseline::new());
    };
    let commit = project.commit(tip)?;
    let paths = Provenance::parse(&commit.message)
        .map(|recorded| recorded.once)
        .unwrap_or_default();
    if paths.is_empty() {
        return Ok(Baseline::new());
    }
    let entries: BTreeMap<_, _> = project
        .list_tree(commit.tree)?
        .into_iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect();
    let mut files = Baseline::new();
    for path in paths {
        crate::render::validate_file_path(&path)?;
        let entry = entries.get(&path).ok_or_else(|| OpError::OnceMissing {
            path: path.clone(),
            revision: tip,
        })?;
        files.insert(
            path.clone(),
            Rendered {
                source: path.clone(),
                path,
                // History alone cannot prove that today's source still owns the captured bytes.
                origin: Origin::Retained,
                content: project.read_blob(entry.oid)?,
                mode: entry.mode,
                templated: false,
            },
        );
    }
    Ok(files)
}
