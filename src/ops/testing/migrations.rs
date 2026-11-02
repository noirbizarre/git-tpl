//! Real project histories for declarative migration cases (ADR-046).

use super::*;
use crate::git::{MergeOutcome, TreeEntry};
use crate::ops::{self, AppliedMigration, Render, UpdateOutcome};

/// A prior template revision and committed project-local customisations.
#[derive(Debug, Clone, PartialEq)]
pub struct MigrationCase {
    /// Old source reference, resolved before initialization.
    pub from: String,
    /// Explicit committed destination, or the suite's captured destination.
    pub to: Option<String>,
    /// Old supplied answers, before defaults are recorded by init.
    pub answers: BTreeMap<String, Value>,
    /// Literal text replacements and additions on the project side.
    pub files: BTreeMap<String, String>,
    /// Existing project files to delete before updating.
    pub absent: Vec<String>,
}

impl MigrationCase {
    pub(super) fn parse(
        value: &Value,
        answers: &BTreeMap<String, Value>,
        shape: &impl Fn(String) -> TestingError,
    ) -> Result<Self, TestingError> {
        let Value::Table(table) = value else {
            return Err(shape("`migration` must be a table.".into()));
        };
        for key in table.keys() {
            if !matches!(key.as_str(), "from" | "to" | "answers" | "files" | "absent") {
                let hint = closest(key, ["from", "to", "answers", "files", "absent"])
                    .map(|near| format!(" Did you mean `{near}`?"))
                    .unwrap_or_default();
                return Err(shape(format!("unknown migration key `{key}`.{hint}")));
            }
        }
        let reference = |key: &str| match table.get(key) {
            Some(Value::String(value)) if !value.trim().is_empty() => Ok(value.clone()),
            _ => Err(shape(format!(
                "`migration.{key}` must be a nonempty Git reference."
            ))),
        };
        let from = reference("from")?;
        let to = table.get("to").map(|_| reference("to")).transpose()?;
        let answers = match table.get("answers") {
            None => answers.clone(),
            Some(Value::Table(answers)) => answers.clone(),
            _ => return Err(shape("`migration.answers` must be a table.".into())),
        };
        let files = string_map(table.get("files"), "migration.files", shape)?;
        let absent = string_array(table.get("absent"), "migration.absent", shape)?;
        for path in files.keys().chain(&absent) {
            // Tree edits must never target Git's metadata or the attachment owned by init.
            if path.is_empty()
                || path.contains('\\')
                || path.split('/').any(|part| {
                    part.is_empty()
                        || matches!(part, "." | "..")
                        || part.eq_ignore_ascii_case(".git")
                })
                || path.contains(':')
                || path == crate::config::CONFIG_PATH
                || crate::config::CONFIG_PATH.starts_with(&format!("{path}/"))
                || path.starts_with(&format!("{}/", crate::config::CONFIG_PATH))
            {
                return Err(shape(format!(
                    "unsafe migration project path `{path}`; use a project-relative file path outside Git metadata and attachment configuration."
                )));
            }
        }
        for path in files.keys() {
            if absent.contains(path)
                || files
                    .keys()
                    .any(|other| other.starts_with(&format!("{path}/")))
            {
                return Err(shape(format!(
                    "ambiguous migration project edit at `{path}`; writes must not overlap deletions or other writes."
                )));
            }
        }
        if absent.iter().collect::<BTreeSet<_>>().len() != absent.len() {
            return Err(shape("`migration.absent` contains duplicate paths.".into()));
        }
        Ok(Self {
            from,
            to,
            answers,
            files,
            absent,
        })
    }
}

fn render_captured<'a>(
    template: &'a Resolved,
    source: &str,
    project: &dyn GitBackend,
    cwd: &Path,
    supplied: crate::answers::SuppliedAnswers,
    user: &UserConfig,
    trusted: bool,
) -> Result<Render<&'a Resolved>, OpError> {
    // Historical endpoints can declare different sources; suite-target decisions cannot be replayed here.
    let decisions = declared_remotes(&template.manifest.data)
        .into_iter()
        .map(|request| {
            (
                request.name,
                if trusted {
                    Decision::Allow
                } else {
                    Decision::Skip
                },
            )
        })
        .collect();
    let rendered = render_resolved(
        template,
        source,
        Some((project, cwd)),
        supplied,
        user,
        Answering::defaults(),
        Trust::decided(decisions),
    )?;
    Ok(Render {
        template,
        tree: crate::render::write_tree(project, &rendered.files)?,
        context: rendered.context,
        provenance: rendered.provenance,
        ignored_answers: rendered.ignored_answers,
    })
}

fn local_edits(project: &dyn GitBackend, migration: &MigrationCase) -> Result<(), OpError> {
    let head = project.head_commit()?.ok_or_else(missing_head)?;
    let mut entries: BTreeMap<_, _> = project
        .list_tree(project.commit_tree(head)?)?
        .into_iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect();
    for path in &migration.absent {
        if entries.remove(path).is_none() {
            return Err(TestingError::CaseShape {
                path: path.clone(),
                reason: "`migration.absent` must name a file in the old project.".into(),
            }
            .into());
        }
    }
    for (path, text) in &migration.files {
        // Preserve executable regular files, but replace links rather than following them.
        let mode = entries
            .get(path)
            .map(|entry| entry.mode)
            .filter(|mode| *mode == FileMode::BlobExecutable)
            .unwrap_or(FileMode::Blob);
        entries.insert(
            path.clone(),
            TreeEntry {
                path: path.clone(),
                mode,
                oid: project.write_blob(text.as_bytes())?,
            },
        );
    }
    let tree = project.build_tree(&entries.into_values().collect::<Vec<_>>())?;
    if tree != project.commit_tree(head)? {
        let commit = project.create_commit(tree, &[head], "tpl test: project customisations")?;
        project.merge(commit, "tpl test: project customisations", true, &[])?;
    }
    Ok(())
}

fn check_migrations(expect: &Expect, migrations: &[AppliedMigration], failures: &mut Vec<Failure>) {
    let paths: Vec<_> = migrations
        .iter()
        .map(|migration| migration.path.clone())
        .collect();
    if let Some(expected) = &expect.migrations
        && expected != &paths
    {
        failures.push(Failure::MigrationAssertion {
            assertion: "expect.migrations".into(),
            path: None,
            message: format!("expected {expected:?}, discovered {paths:?}"),
        });
    }
    for (path, needles) in &expect.messages {
        let message = migrations
            .iter()
            .find(|migration| &migration.path == path)
            .and_then(|migration| migration.message.as_deref());
        for needle in needles {
            if message.is_none_or(|message| !message.contains(needle)) {
                failures.push(Failure::MigrationAssertion {
                    assertion: "expect.messages".into(),
                    path: Some(path.clone()),
                    message: format!("expected message containing {needle:?}, got {message:?}"),
                });
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_case(
    suite: &Resolved,
    source: &str,
    root: Option<&str>,
    tests_dir: &str,
    case: &Case,
    write: bool,
    run_commands: bool,
    resolution_user: &UserConfig,
    user: &UserConfig,
    color: bool,
    progress: &mut dyn Progress,
) -> Result<CaseOutcome, OpError> {
    let migration = case.migration.as_ref().expect("migration dispatch");
    let sandbox = tempfile::tempdir().map_err(|error| TestingError::SandboxFailed {
        case: case.name.clone(),
        reason: error.to_string(),
    })?;
    let cwd = sandbox.path();
    // Keep this handle: reopening with ambient configuration would undo isolation.
    let project = init_git_sandbox(
        cwd,
        case.git.as_ref().unwrap_or(&GitSandbox::default_identity()),
    )?;
    let template_root = suite.repo.workdir()?;
    let enabled = run_commands && !write;
    let mut failures = Vec::new();
    let mut commands_run = 0;
    let mut phase = "setup";
    let mut paths = Vec::new();
    let execute =
        |step, list: &CommandList, failures: &mut Vec<Failure>, progress: &mut dyn Progress| {
            if !enabled {
                return 0;
            }
            let mut env = git_isolation_env(cwd);
            env.extend(list.env.clone());
            execute_commands(
                step,
                &list.commands,
                cwd,
                &template_root,
                &env,
                &case.answers,
                step != CommandStep::Finally,
                failures,
                &case.name,
                color,
                progress,
            )
        };
    commands_run += execute(
        CommandStep::Before,
        &case.commands.before,
        &mut failures,
        progress,
    );
    // Keep every fallible step inside the closure so finally runs even after a snapshot I/O error.
    let result = (|| -> Result<(usize, SnapshotOutcome, Vec<SnapshotChange>), OpError> {
        if !failures.is_empty() {
            return Ok((0, SnapshotOutcome::None, Vec::new()));
        }
        let resolve_endpoint = |reference: &str| {
            let mut trust = Trust::refuse();
            resolve::resolve(
                ops::Request {
                    source,
                    reference: Some(reference),
                    root,
                    dirty: false,
                },
                resolution_user,
                &suite.access,
                &mut trust,
            )
        };
        let old = resolve_endpoint(&migration.from)?;
        let target = migration.to.as_deref().map(resolve_endpoint).transpose()?;
        let target = target.as_ref().unwrap_or(suite);
        progress.case_status(&case.name, Status::Rendering);
        phase = "init";
        // `test` defaults to `.`, which has no derivable template id. Identity is
        // private to this one scratch repository and must not depend on its temp path.
        let initialized = ops::init_using(
            &project,
            cwd,
            source,
            Some(old.reference.clone()),
            Some("migration-test"),
            crate::answers::typed(migration.answers.clone()),
            true,
            false,
            true,
            |_, supplied| render_captured(&old, source, &project, cwd, supplied, user, case.trust),
        )?;
        if let Some(MergeOutcome::Conflicted { paths }) = initialized.merge {
            for path in paths {
                failures.push(conflict(path));
            }
            return Ok((0, SnapshotOutcome::None, Vec::new()));
        }
        phase = "local-edits";
        local_edits(&project, migration)?;
        phase = "update";
        let updated = ops::update_using(
            &project,
            cwd,
            Some(&target.reference),
            crate::answers::typed(case.answers.clone()),
            true,
            |config, supplied| {
                render_captured(
                    target,
                    &config.template.source,
                    &project,
                    cwd,
                    supplied,
                    user,
                    case.trust,
                )
            },
        );
        let updated = match updated {
            Ok(updated) => updated,
            Err(error) if !write => {
                classify_render_error(error, &case.expect.error, &mut failures);
                return Ok((0, SnapshotOutcome::None, Vec::new()));
            }
            Err(error) => return Err(error),
        };
        let (commit, migrations, config_changed) = match updated {
            UpdateOutcome::Updated {
                commit,
                migrations,
                answers_changed,
                composed_changed,
                ..
            } => (commit, migrations, answers_changed || composed_changed),
            UpdateOutcome::UpToDate {
                answers_changed,
                composed_changed,
                ..
            } => (
                project
                    .resolve_ref(&initialized.id.ref_name())?
                    .ok_or_else(missing_head)?,
                Vec::new(),
                answers_changed || composed_changed,
            ),
        };
        paths = migrations
            .iter()
            .map(|migration| migration.path.clone())
            .collect();
        if !write && let Some(code) = &case.expect.error {
            failures.push(Failure::ExpectedError { code: code.clone() });
            return Ok((0, SnapshotOutcome::None, Vec::new()));
        }
        phase = "merge";
        // Update intentionally leaves changed attachment answers unstaged. Commit only in the scratch project.
        if config_changed {
            project.commit_attachment("tpl test: record updated answers")?;
        }
        if let MergeOutcome::Conflicted { paths } =
            project.merge(commit, "tpl test: merge updated template", true, &[])?
        {
            for path in paths {
                failures.push(conflict(path));
            }
            return Ok((0, SnapshotOutcome::None, Vec::new()));
        }
        let head = project.head_commit()?.ok_or_else(missing_head)?;
        let rendered = project
            .list_tree(project.commit_tree(head)?)?
            .into_iter()
            .filter(|entry| entry.path != crate::config::CONFIG_PATH)
            .map(|entry| {
                Ok(Rendered {
                    source: entry.path.clone(),
                    path: entry.path,
                    origin: crate::render::Origin::Leaf,
                    content: project.read_blob(entry.oid)?,
                    mode: entry.mode,
                    templated: false,
                })
            })
            .collect::<Result<Vec<_>, GitError>>()?;
        phase = "assertions";
        commands_run += execute(
            CommandStep::Rendered,
            &case.commands.rendered,
            &mut failures,
            progress,
        );
        if !write {
            check(&case.expect, &rendered, &mut failures);
            check_migrations(&case.expect, &migrations, &mut failures);
        }
        let (snapshot, changes) =
            snapshot_step(suite, tests_dir, case, &rendered, write, &mut failures)?;
        commands_run += execute(
            CommandStep::After,
            &case.commands.after,
            &mut failures,
            progress,
        );
        Ok((rendered.len(), snapshot, changes))
    })();
    commands_run += execute(
        CommandStep::Finally,
        &case.commands.finally,
        &mut failures,
        progress,
    );
    let (files, snapshot, snapshot_changes) = match result {
        Ok(value) => value,
        Err(error) => {
            classify_render_error(error, &None, &mut failures);
            (0, SnapshotOutcome::None, Vec::new())
        }
    };
    let failures = failures
        .into_iter()
        .map(|failure| Failure::Migration {
            from: migration.from.clone(),
            to: migration
                .to
                .clone()
                .unwrap_or_else(|| suite.reference.clone()),
            phase: phase.into(),
            migrations: paths.clone(),
            failure: Box::new(failure),
        })
        .collect();
    Ok(CaseOutcome {
        name: case.name.clone(),
        path: case.path.clone(),
        failures,
        files,
        snapshot,
        snapshot_changes,
        commands_run,
        duration_ms: 0,
    })
}

fn conflict(path: String) -> Failure {
    Failure::MigrationAssertion {
        assertion: "merge".into(),
        path: Some(path),
        message: "Git reported an unresolved merge conflict".into(),
    }
}

fn missing_head() -> TestingError {
    TestingError::SandboxFailed {
        case: "migration".into(),
        reason: "the initialized scratch project has no HEAD commit".into(),
    }
}
