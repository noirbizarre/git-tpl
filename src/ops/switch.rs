//! Change the source of an attachment while retaining its rendered merge base.

use super::*;

/// A completed source transition. Configuration is saved, but never staged.
pub struct SwitchOutcome {
    /// The source previously attached to the project.
    pub previous_source: String,
    /// The persistently selected target source.
    pub source: String,
    /// Whether the target extends the previous source, directly or transitively.
    pub inherited: bool,
    /// The existing attachment identity, independent of the new source.
    pub id: TemplateId,
    /// The published rendered tip (possibly unchanged).
    pub commit: Oid,
    /// Whether a new rendering commit was published.
    pub changed: bool,
    /// Changes from the previous rendering.
    pub changes: Vec<Change>,
    /// The requested target revision name.
    pub reference: String,
    /// The resolved target commit.
    pub revision: Oid,
    /// Whether the target's working tree was rendered.
    pub dirty: bool,
    /// Applicable recorded answers accepted without an explicit override.
    pub reused_answers: Vec<String>,
    /// Recorded keys not reused (including explicit replacements and inactive keys).
    pub skipped_answers: Vec<String>,
    /// Explicit inputs naming no target question.
    pub ignored_answers: Vec<String>,
    /// Ignored paths relevant to this rendering.
    pub ignored: Vec<String>,
}

/// Render a replacement template onto the existing append-only attachment history.
#[allow(clippy::too_many_arguments)]
pub fn switch(
    project: &dyn GitBackend,
    project_root: &Path,
    source: &str,
    reference: Option<String>,
    supplied: crate::answers::SuppliedAnswers,
    dirty: bool,
    strict_answers: bool,
    user: &UserConfig,
    access: &Access,
    answering: Answering<'_>,
    mut trust: Trust<'_>,
) -> Result<SwitchOutcome, OpError> {
    let _operation = project.lock_template_operation()?;
    let initial = Config::load(project_root)?;
    let id = TemplateId::resolve(&initial.template.source, initial.template.id.as_deref())?;
    let ref_name = id.ref_name();
    // An orphan would lose the distinction between project customizations and
    // old template output, precisely when switching needs that distinction most.
    let tip = project
        .resolve_ref(&ref_name)?
        .ok_or_else(|| OpError::NoRenderedRef {
            ref_name: ref_name.clone(),
        })?;
    let previous = project.commit(tip)?;
    let mut template = resolve::resolve(
        Request {
            source,
            reference: reference.as_deref(),
            // Defer root selection until inheritance decides whether the old
            // override survives. The leaf's default root may not exist at all.
            root: Some("."),
            dirty,
        },
        user,
        access,
        &mut trust,
    )?;
    let current_location = crate::config::source_location(&initial.template.source, project_root);
    let inherited = template.extends_provenance().iter().any(|ancestor| {
        crate::git::cache::identity(&ancestor.source)
            == crate::git::cache::identity(&current_location)
    });
    let mut config = if inherited {
        initial.clone()
    } else {
        Config::new(source, reference.clone())
    };
    config.template.source = source.into();
    config.template.r#ref = reference;
    // The ID names this project's rendering history, not the replacement source.
    config.template.id = Some(id.as_str().into());
    config.template.rebase_local(project_root)?;
    let root = config
        .template
        .root
        .as_deref()
        .unwrap_or(&template.manifest.root)
        .to_string();
    template.select_root(&root)?;

    let explicit_keys: std::collections::BTreeSet<_> = supplied.keys().cloned().collect();
    let mut rendered = render_resolved_preparing(
        &template,
        source,
        Some((project, project_root)),
        supplied,
        user,
        answering,
        trust,
        None,
        Preparation::Switch(&initial.answers),
        |_| Ok(()),
    )?;
    enforce_strict_answers(
        strict_answers,
        &rendered.ignored_answers,
        template.manifest.questions.keys().cloned(),
    )?;
    rendered.provenance.source = config.template.source.clone();
    let reused_answers: Vec<_> = initial
        .answers
        .iter()
        .filter(|(key, value)| {
            !explicit_keys.contains(*key) && rendered.context.answers().get(*key) == Some(*value)
        })
        .map(|(key, _)| key.clone())
        .collect();
    let skipped_answers = initial
        .answers
        .keys()
        .filter(|key| !reused_answers.contains(key))
        .cloned()
        .collect();
    // Preserve inherited configuration, including currently gated answers; an
    // unrelated source must not inherit stale answer namespaces or child locks.
    config.answers.extend(rendered.context.answers().clone());
    config.composed = rendered
        .provenance
        .composed
        .iter()
        .map(ComposedRevision::lock)
        .collect();
    config.to_toml()?;
    let tree = write_tree(project, &rendered.files)?;
    let changes = project.diff_trees(Some(previous.tree), tree, &[])?;
    let changed = tree != previous.tree
        || initial.template != config.template
        || Provenance::parse(&previous.message).is_none_or(|old| {
            old.source.as_deref() != Some(rendered.provenance.source.as_str())
                || old.revision != Some(template.revision)
                || old.composed != rendered.provenance.composed
                || old.dirty != template.dirty
        });
    if Config::load(project_root)? != initial {
        return Err(ConfigError::Changed {
            path: Config::path_in(project_root),
        }
        .into());
    }
    let commit = if changed {
        project.create_commit(tree, &[tip], &rendered.provenance.to_message())?
    } else {
        tip
    };
    // Even an unchanged rendering must not persist config from a stale ref snapshot.
    project.compare_and_set_ref(&ref_name, Some(tip), commit, "tpl: switch")?;
    if config != initial {
        config.save(project_root)?;
    }
    Ok(SwitchOutcome {
        previous_source: initial.template.source,
        source: config.template.source,
        inherited,
        id,
        commit,
        changed,
        changes,
        reference: template.reference,
        revision: template.revision,
        dirty: template.dirty,
        reused_answers,
        skipped_answers,
        ignored_answers: rendered.ignored_answers,
        ignored: template.ignored,
    })
}
