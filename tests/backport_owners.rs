//! Ownership is proved against real historical repositories; applicability is adjudicated by Git.

mod common;

use common::{Repo, tpl};

struct Owners {
    base: Repo,
    child: Repo,
    leaf: Repo,
    project: Repo,
}

impl Owners {
    fn new(modules: &str) -> Self {
        let base = Repo::init("base");
        base.write("template.toml", "name = 'base'\nroot = 'base-root'\n");
        base.write("base-root/inherited.txt", "inherited original\n");
        base.commit_all("base");
        let child = Repo::init("child");
        child.write("template.toml", "name = 'child'\nroot = 'payload'\nstrict = true\n[questions.module_name]\ntype = 'string'\n[questions.module_path]\ntype = 'string'\n");
        child.write(
            "payload/{{ module_path }}/module.txt.jinja",
            "{{ module_name }}\noriginal\n",
        );
        child.commit_all("child");
        let leaf = Repo::init("leaf");
        leaf.write("template.toml", &format!(
            "name = 'leaf'\n[extends]\nsource = '{}'\nrev = '{}'\n[questions.modules]\ntype = 'list'\ndefault = []\n[[compose]]\nsource = '{}'\nref = 'main'\nfrom = 'modules'\n",
            base.path.display(), base.rev_parse("HEAD"), child.path.display(),
        ));
        leaf.write("template/leaf.txt", "leaf original\n");
        leaf.commit_all("leaf");
        let project = Repo::init("project");
        tpl(
            &project,
            &[
                "init",
                leaf.path.to_str().unwrap(),
                "--defaults",
                "--answer",
                &format!("modules={modules}"),
            ],
        )
        .success();
        Self {
            base,
            child,
            leaf,
            project,
        }
    }

    fn edit_all(&self) {
        self.project.write("leaf.txt", "leaf fixed\n");
        self.project.write("inherited.txt", "inherited fixed\n");
        self.project.write("modules/one/module.txt", "one\nfixed\n");
    }
}

fn apply_in_clone(source: &Repo, commit: &str, patch: &str) -> (tempfile::TempDir, Repo) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("checkout");
    source.git(&[
        "clone",
        "-q",
        "-c",
        "core.autocrlf=false",
        "-c",
        "core.eol=lf",
        source.path.to_str().unwrap(),
        path.to_str().unwrap(),
    ]);
    let clone = Repo::at(path);
    clone.git(&["config", "user.name", "Test"]);
    clone.git(&["config", "user.email", "test@example.invalid"]);
    clone.git(&["config", "commit.gpgsign", "false"]);
    clone.git(&["checkout", "--detach", commit]);
    clone.write("change.patch", patch);
    clone.git(&["am", "change.patch"]);
    (directory, clone)
}

#[test]
fn mixed_owners_publish_separate_applicable_mailboxes_without_touching_any_repository() {
    let world = Owners::new("[\"one\"]");
    world.edit_all();
    let repositories = [&world.project, &world.leaf, &world.base, &world.child];
    let before: Vec<_> = repositories
        .iter()
        .map(|repo| (repo.working_state(), repo.git(&["show-ref"])))
        .collect();
    assert_eq!(
        tpl(
            &world.project,
            &["--json", "backport", "-o", "unsafe.patch"]
        )
        .failure()
        .error_code(),
        "tpl::backport::multiple_owners"
    );
    assert!(!world.project.exists("unsafe.patch"));
    let raw = tpl(&world.project, &["backport"]).failure();
    assert!(raw.stdout.is_empty());
    let output = tpl(&world.project, &["--json", "backport"])
        .success()
        .json();
    assert_eq!(output["result"], "grouped");
    let owners = output["owners"].as_array().unwrap();
    assert_eq!(owners.len(), 3);
    for owner in owners {
        let source = repositories
            .iter()
            .find(|repo| {
                std::fs::canonicalize(
                    tpl::config::source_location(
                        owner["template"].as_str().unwrap(),
                        &world.project.path,
                    )
                    .as_ref(),
                )
                .unwrap()
                    == repo.path.canonicalize().unwrap()
            })
            .unwrap_or_else(|| panic!("unknown owner: {owner}"));
        let (_directory, clone) = apply_in_clone(
            source,
            owner["revision"]["commit"].as_str().unwrap(),
            owner["patch"].as_str().unwrap(),
        );
        let path = owner["files"][0]["source"].as_str().unwrap();
        assert!(clone.read(path).contains("fixed"));
        assert!(
            !owner["applyCommand"]
                .as_str()
                .unwrap()
                .contains("backport |")
        );
    }
    for (repo, state) in repositories.iter().zip(&before) {
        assert_eq!(repo.working_state(), state.0);
        assert_eq!(repo.git(&["show-ref"]), state.1);
    }

    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("review ' patches");
    let args = [
        "--json",
        "backport",
        "--output-dir",
        destination.to_str().unwrap(),
    ];
    tpl(&world.project, &args).success();
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(destination.join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["version"], 1);
    for owner in manifest["owners"].as_array().unwrap() {
        assert!(owner.get("patch").is_none());
        assert!(
            destination
                .join(owner["mailbox"].as_str().unwrap())
                .is_file()
        );
        assert!(owner["applyCommand"].as_str().unwrap().contains("'\\''"));
    }
    assert_eq!(
        tpl(&world.project, &args).failure().error_code(),
        "tpl::backport::output_write"
    );
    assert!(destination.join("manifest.json").is_file());
}

#[test]
fn shared_sources_deduplicate_identical_edits_and_refuse_conflicts_or_unselected_effects() {
    let world = Owners::new("[\"one\", \"two\"]");
    world
        .project
        .write("modules/one/module.txt", "one\nfixed\n");
    assert_eq!(
        tpl(&world.project, &["--json", "backport", "modules/one"])
            .failure()
            .error_code(),
        "tpl::backport::shared_source"
    );
    world
        .project
        .write("modules/two/module.txt", "two\ndifferent\n");
    assert_eq!(
        tpl(&world.project, &["--json", "backport"])
            .failure()
            .error_code(),
        "tpl::backport::source_conflict"
    );
    world
        .project
        .write("modules/two/module.txt", "two\nfixed\n");
    let output = tpl(&world.project, &["--json", "backport"])
        .success()
        .json();
    assert_eq!(output["files"].as_array().unwrap().len(), 1);
    assert_eq!(
        output["patch"]
            .as_str()
            .unwrap()
            .matches("diff --git")
            .count(),
        1
    );
    assert_eq!(output["insertions"], 1);
    assert_eq!(
        output["sourceOutputs"]["payload/{{ module_path }}/module.txt.jinja"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let (_directory, clone) = apply_in_clone(
        &world.child,
        output["revision"]["commit"].as_str().unwrap(),
        output["patch"].as_str().unwrap(),
    );
    assert_eq!(
        clone.read("payload/{{ module_path }}/module.txt.jinja"),
        "{{ module_name }}\nfixed\n"
    );
}

#[test]
fn a_composed_inherited_source_uses_its_ancestor_root_and_its_module_environment() {
    let world = Owners::new("[]");
    let ancestor = Repo::init("child-base");
    ancestor.write("template.toml", "name = 'child-base'\nroot = 'sources'\n");
    ancestor.write(
        "sources/{{ module_path }}/inherited.txt.jinja",
        "{{ module_name }}\noriginal\n",
    );
    ancestor.commit_all("child ancestor");
    let manifest = world.child.read("template.toml");
    world.child.write(
        "template.toml",
        &format!(
            "{manifest}\n[extends]\nsource = '{}'\nrev = '{}'\n",
            ancestor.path.display(),
            ancestor.rev_parse("HEAD")
        ),
    );
    world.child.commit_all("inherit child source");
    tpl(
        &world.project,
        &["update", "--defaults", "--answer", "modules=[\"one\"]"],
    )
    .success();
    tpl(&world.project, &["merge"]).success();
    world
        .project
        .write("modules/one/inherited.txt", "one\nfixed\n");
    let before = ancestor.working_state();
    let output = tpl(
        &world.project,
        &["--json", "backport", "modules/one/inherited.txt"],
    )
    .success()
    .json();
    assert_eq!(
        output["template"],
        ancestor.path.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(
        output["files"][0]["source"],
        "sources/{{ module_path }}/inherited.txt.jinja"
    );
    let (_directory, clone) = apply_in_clone(
        &ancestor,
        output["revision"]["commit"].as_str().unwrap(),
        output["patch"].as_str().unwrap(),
    );
    assert_eq!(
        clone.read("sources/{{ module_path }}/inherited.txt.jinja"),
        "{{ module_name }}\nfixed\n"
    );
    assert_eq!(ancestor.working_state(), before);
}

#[test]
fn child_backports_keep_the_recorded_commit_after_its_branch_moves() {
    let world = Owners::new("[\"one\"]");
    let original = world.child.rev_parse("HEAD");
    world
        .child
        .write(
            "payload/{{ module_path }}/module.txt.jinja",
            "new upstream: {{ module_name }}\noriginal\n",
        )
        .commit_all("advance child branch");
    world
        .project
        .write("modules/one/module.txt", "one\nfixed\n");
    let output = tpl(&world.project, &["--json", "backport"])
        .success()
        .json();
    assert_eq!(output["revision"]["commit"], original);
    assert_eq!(output["revision"]["reference"], "main");
    let (_directory, clone) =
        apply_in_clone(&world.child, &original, output["patch"].as_str().unwrap());
    assert_eq!(
        clone.read("payload/{{ module_path }}/module.txt.jinja"),
        "{{ module_name }}\nfixed\n"
    );
}

#[test]
fn moved_ancestor_tags_are_refused_even_when_their_rendered_bytes_are_identical() {
    let base = Repo::init("base");
    base.write("template.toml", "name = 'base'\n");
    base.write("template/file.txt", "original\n")
        .commit_all("base");
    base.git(&["tag", "v1"]);
    let leaf = Repo::init("leaf");
    leaf.write(
        "template.toml",
        &format!(
            "name = 'leaf'\n[extends]\nsource = '{}'\nrev = 'v1'\n",
            base.path.display()
        ),
    );
    leaf.commit_all("leaf");
    let project = Repo::init("project");
    tpl(
        &project,
        &["init", leaf.path.to_str().unwrap(), "--defaults"],
    )
    .success();
    base.write("not-rendered.txt", "metadata\n")
        .commit_all("same output, different source revision");
    base.git(&["tag", "-f", "v1"]);
    project.write("file.txt", "fixed\n");
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("patches");
    assert_eq!(
        tpl(
            &project,
            &[
                "--json",
                "backport",
                "--output-dir",
                output.to_str().unwrap()
            ]
        )
        .failure()
        .error_code(),
        "tpl::backport::history"
    );
    assert!(!output.exists());
}

#[test]
fn named_hunks_emit_only_the_selected_owner_and_empty_runs_create_no_directory() {
    let world = Owners::new("[\"one\"]");
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("empty");
    tpl(
        &world.project,
        &["backport", "--output-dir", destination.to_str().unwrap()],
    )
    .success();
    assert!(!destination.exists());
    world.edit_all();
    let listing = tpl(&world.project, &["--json", "backport", "--list-hunks"])
        .success()
        .json();
    assert_eq!(listing["plan"].as_array().unwrap().len(), 3);
    let spec = listing["plan"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["rendered"] == "inherited.txt")
        .unwrap()["hunks"][0]["spec"]
        .as_str()
        .unwrap();
    let output = tpl(&world.project, &["--json", "backport", "--hunk", spec])
        .success()
        .json();
    assert_eq!(output["result"], "patched");
    assert_eq!(
        output["template"],
        world.base.path.canonicalize().unwrap().to_str().unwrap()
    );
}

fn rewrite_provenance(project: &Repo, rewrite: impl FnOnce(String) -> String) {
    let reference = project.git(&["for-each-ref", "--format=%(refname)", "refs/tpl/"]);
    let reference = reference.trim();
    let tip = project.rev_parse(reference);
    let tree = project.rev_parse(&format!("{reference}^{{tree}}"));
    let message = rewrite(project.git(&["show", "-s", "--format=%B", reference]));
    let commit = project.git(&["commit-tree", &tree, "-p", &tip, "-m", &message]);
    project.git(&["update-ref", reference, commit.trim(), &tip]);
}

fn without_child_ancestry(message: String) -> String {
    message
        .lines()
        .map(|line| {
            if let Some(value) = line.strip_prefix("Template-Compose: ") {
                let mut value: serde_json::Value = serde_json::from_str(value).unwrap();
                value.as_object_mut().unwrap().remove("extends");
                value.as_object_mut().unwrap().remove("ancestryVersion");
                format!("Template-Compose: {value}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn malformed_duplicate_missing_and_source_mismatched_history_never_publish_artifacts() {
    for kind in ["malformed", "duplicate", "missing", "source", "version"] {
        let world = Owners::new("[\"one\"]");
        world
            .project
            .write("modules/one/module.txt", "one\nfixed\n");
        rewrite_provenance(&world.project, |message| {
            let child = message
                .lines()
                .find(|line| line.starts_with("Template-Compose: "))
                .unwrap();
            match kind {
                "malformed" => message.replace(child, "Template-Compose: {broken}"),
                "duplicate" => format!("{message}\n{child}"),
                "missing" => message.replace(child, ""),
                "source" => message
                    .lines()
                    .map(|line| {
                        if line.starts_with("Template-Source: ") {
                            "Template-Source: /wrong/source"
                        } else {
                            line
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                "version" => message.replace("\"ancestryVersion\":1", "\"ancestryVersion\":99"),
                _ => unreachable!(),
            }
        });
        let repositories = [&world.project, &world.leaf, &world.base, &world.child];
        let before: Vec<_> = repositories
            .iter()
            .map(|repo| (repo.working_state(), repo.git(&["show-ref"])))
            .collect();
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("refused");
        let output = tpl(
            &world.project,
            &[
                "--json",
                "backport",
                "--output-dir",
                destination.to_str().unwrap(),
            ],
        )
        .failure();
        assert_eq!(
            output.error_code(),
            "tpl::backport::history",
            "{kind}: {}",
            output.all()
        );
        assert!(!destination.exists());
        for (repo, state) in repositories.iter().zip(before) {
            assert_eq!(repo.working_state(), state.0);
            assert_eq!(repo.git(&["show-ref"]), state.1);
        }
    }
}

#[test]
fn legacy_child_ancestry_requires_full_immutable_pins_but_new_evidence_supports_tags() {
    for pinned in [false, true] {
        let world = Owners::new("[]");
        let ancestor = Repo::init("child-base");
        ancestor.write("template.toml", "name = 'child-base'\n");
        ancestor
            .write("template/unused.txt", "ancestor\n")
            .commit_all("ancestor");
        ancestor.git(&["tag", "v1"]);
        let pin = if pinned {
            ancestor.rev_parse("HEAD")
        } else {
            "v1".to_string()
        };
        let manifest = world.child.read("template.toml");
        world
            .child
            .write(
                "template.toml",
                &format!(
                    "{manifest}\n[extends]\nsource = '{}'\nrev = '{pin}'\n",
                    ancestor.path.display()
                ),
            )
            .commit_all("inherit ancestor");
        tpl(
            &world.project,
            &["update", "--defaults", "--answer", "modules=[\"one\"]"],
        )
        .success();
        tpl(&world.project, &["merge"]).success();
        world
            .project
            .write("modules/one/module.txt", "one\nfixed\n");
        tpl(&world.project, &["--json", "backport"]).success();
        rewrite_provenance(&world.project, without_child_ancestry);
        if pinned {
            tpl(&world.project, &["--json", "backport"]).success();
        } else {
            assert_eq!(
                tpl(&world.project, &["--json", "backport"])
                    .failure()
                    .error_code(),
                "tpl::backport::history"
            );
        }
    }
}

#[test]
fn legacy_child_evidence_alone_does_not_manufacture_an_update_commit() {
    let world = Owners::new("[\"one\"]");
    rewrite_provenance(&world.project, without_child_ancestry);
    let reference = world
        .project
        .git(&["for-each-ref", "--format=%(refname)", "refs/tpl/"]);
    let tip = world.project.rev_parse(reference.trim());
    let before = world.project.working_state();
    let preview = tpl(
        &world.project,
        &["--json", "update", "--defaults", "--dry-run"],
    )
    .success()
    .json();
    assert_eq!(preview["result"], "upToDate");
    assert_eq!(world.project.working_state(), before);
    assert_eq!(world.project.rev_parse(reference.trim()), tip);
    let updated = tpl(&world.project, &["--json", "update", "--defaults"])
        .success()
        .json();
    assert_eq!(updated["result"], "upToDate");
    assert_eq!(world.project.working_state(), before);
    assert_eq!(world.project.rev_parse(reference.trim()), tip);
    world
        .project
        .write("modules/one/module.txt", "one\nfixed\n");
    tpl(&world.project, &["--json", "backport"]).success();
}

#[cfg(unix)]
#[test]
fn output_directory_links_are_refused_without_touching_their_target() {
    let world = Owners::new("[\"one\"]");
    world.edit_all();
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target");
    std::fs::create_dir(&target).unwrap();
    let link = directory.path().join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert_eq!(
        tpl(
            &world.project,
            &["--json", "backport", "--output-dir", link.to_str().unwrap()]
        )
        .failure()
        .error_code(),
        "tpl::backport::output_write"
    );
    assert_eq!(std::fs::read_dir(target).unwrap().count(), 0);
    assert!(link.is_symlink());
}

#[test]
fn transitive_inherited_sources_are_proved_with_leaf_answers_and_overridden_partials() {
    let base = Repo::init("base");
    base.write(
        "template.toml",
        "name = 'base'\nroot = 'sources'\n[questions.name]\ntype = 'string'\ndefault = 'base'\n",
    );
    base.write(
        "labels.jinja",
        "{% macro label(name) %}base {{ name }}{% endmacro %}",
    );
    base.write(
        "sources/file.txt.jinja",
        "{% from 'labels.jinja' import label %}{{ label(name) }}\noriginal\n",
    )
    .commit_all("base");
    let middle = Repo::init("middle");
    middle
        .write(
            "template.toml",
            &format!(
                "name = 'middle'\n[extends]\nsource = '{}'\nrev = '{}'\n",
                base.path.display(),
                base.rev_parse("HEAD")
            ),
        )
        .commit_all("middle");
    let leaf = Repo::init("leaf");
    leaf.write("template.toml", &format!("name = 'leaf'\n[extends]\nsource = '{}'\nrev = '{}'\n[questions.name]\ntype = 'string'\ndefault = 'project'\n", middle.path.display(), middle.rev_parse("HEAD")));
    leaf.write(
        "labels.jinja",
        "{% macro label(name) %}leaf {{ name }}{% endmacro %}",
    )
    .commit_all("leaf overrides");
    let project = Repo::init("project");
    tpl(
        &project,
        &["init", leaf.path.to_str().unwrap(), "--defaults"],
    )
    .success();
    assert_eq!(project.read("file.txt"), "leaf project\noriginal\n");
    // Older top-level trailers used short IDs; the proof resolves them uniquely in their own repositories.
    rewrite_provenance(&project, |message| {
        message
            .lines()
            .map(|line| {
                if let Some(value) = line.strip_prefix("Template-Extends: ") {
                    let (source, commit) = value.rsplit_once('@').unwrap();
                    format!("Template-Extends: {source}@{}", &commit[..7])
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    });
    project.write("file.txt", "leaf project\nfixed\n");
    let output = tpl(&project, &["--json", "backport"]).success().json();
    assert_eq!(
        output["template"],
        base.path.canonicalize().unwrap().to_str().unwrap()
    );
    let (_directory, clone) = apply_in_clone(
        &base,
        output["revision"]["commit"].as_str().unwrap(),
        output["patch"].as_str().unwrap(),
    );
    assert!(
        clone
            .read("sources/file.txt.jinja")
            .contains("{{ label(name) }}\nfixed\n")
    );
}

#[test]
fn one_repository_at_different_base_revisions_requires_selecting_one_revision() {
    let world = Owners::new("[]");
    world.child.git(&["tag", "old"]);
    world
        .child
        .write(
            "payload/{{ module_path }}/module.txt.jinja",
            "{{ module_name }}\nnew original\n",
        )
        .commit_all("second child revision");
    let manifest = world.leaf.read("template.toml");
    world.leaf.write("template.toml", &format!("{manifest}\n[questions.older]\ntype = 'list'\ndefault = ['two']\n[[compose]]\nsource = '{}'\nref = 'old'\nfrom = 'older'\n", world.child.path.display())).commit_all("two child bases");
    tpl(
        &world.project,
        &["update", "--defaults", "--answer", "modules=[\"one\"]"],
    )
    .success();
    tpl(&world.project, &["merge"]).success();
    world
        .project
        .write("modules/one/module.txt", "one\nfixed\n");
    world
        .project
        .write("modules/two/module.txt", "two\nfixed\n");
    assert_eq!(
        tpl(&world.project, &["--json", "backport"])
            .failure()
            .error_code(),
        "tpl::backport::history"
    );
    tpl(&world.project, &["--json", "backport", "modules/one"]).success();
}

#[test]
fn artifact_apply_hints_resolve_relative_outputs_from_the_invocation_directory() {
    let assert_artifact = |command: &serde_json::Value, artifact: &std::path::Path| {
        let command = command.as_str().unwrap();
        let arguments = shlex::split(command).unwrap();
        // Windows may spell the same working directory with or without a verbatim prefix.
        // Check where the quoted argument actually points rather than its display spelling.
        let actual = std::path::Path::new(arguments.last().unwrap())
            .canonicalize()
            .unwrap_or_else(|error| panic!("invalid artifact in `{command}`: {error}"));
        assert_eq!(actual, artifact.canonicalize().unwrap(), "{command}");
    };
    let world = Owners::new("[\"one\"]");
    world.edit_all();
    let cwd = world.project.path.join("modules/one");
    let output = common::tpl_outside(
        &cwd,
        world.project.config_home(),
        &["--json", "backport", "--output-dir", "review"],
    )
    .success()
    .json();
    for owner in output["owners"].as_array().unwrap() {
        let artifact = cwd.join("review").join(owner["mailbox"].as_str().unwrap());
        assert!(artifact.is_file());
        assert_artifact(&owner["applyCommand"], &artifact);
    }
    let output = common::tpl_outside(
        &cwd,
        world.project.config_home(),
        &["--json", "backport", "inherited.txt", "-o", "single.patch"],
    )
    .success()
    .json();
    assert!(cwd.join("single.patch").is_file());
    assert_artifact(&output["applyCommand"], &cwd.join("single.patch"));
}
