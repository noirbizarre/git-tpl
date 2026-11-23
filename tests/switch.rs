//! Source transitions retain real rendered ancestry and reuse only compatible input.

mod common;

use common::{Repo, Template, World, tpl};
use tpl::config::Config;
use tpl::template::Value;

const BASE: &str = "name = 'base'\n[questions.name]\ntype = 'string'\ndefault = 'original'\n";

#[test]
fn primitive_constraints_filter_switch_candidates_but_reject_explicit_invalid_answers() {
    let world = World::with_template(BASE, &[("name.jinja", "{{ name }}")]);
    world.init(&[]).success();
    let replacement = target(
        "name = 'bounded'\n[questions.name]\ntype = 'string'\nmax_length = 3\ndefault = 'new'\n",
        &[("name.jinja", "{{ name }}")],
    );
    let before = world.project.working_state();
    assert_eq!(
        tpl(
            &world.project,
            &[
                "--json",
                "switch",
                &replacement.source(),
                "--defaults",
                "--answer",
                "name=oversized"
            ]
        )
        .failure()
        .error_code(),
        "tpl::eval::string_length"
    );
    assert_eq!(world.project.working_state(), before);
    let result = tpl(
        &world.project,
        &["--json", "switch", &replacement.source(), "--defaults"],
    )
    .success()
    .json();
    assert_eq!(result["reusedAnswers"], serde_json::json!([]));
    assert_eq!(
        Config::load(&world.project.path).unwrap().answers["name"],
        Value::String("new".into())
    );
}

fn target(manifest: &str, files: &[(&str, &str)]) -> Template {
    let repo = Repo::init("replacement");
    repo.write("template.toml", manifest);
    for (path, content) in files {
        repo.write(&format!("template/{path}"), content);
    }
    repo.commit_all("create replacement");
    Template { repo }
}

#[test]
fn direct_and_transitive_children_preserve_answers_config_and_rendered_ancestry() {
    for transitive in [false, true] {
        let world = World::with_template(BASE, &[("name.jinja", "{{ name }}")]);
        world
            .init(&["--answer", "name=personal", "--id", "stable"])
            .success();
        let mut config = Config::load(&world.project.path).unwrap();
        config.template.root = Some("template".into());
        config.answers.insert("obsolete".into(), Value::Bool(true));
        config.save(&world.project.path).unwrap();
        world
            .project
            .commit_all("retain custom attachment settings");
        let child = Template::extending("child", &world.template, "base-v1", "", &[]);
        let leaf = if transitive {
            Template::extending("leaf", &child, "child-v1", "", &[("new", "leaf")])
        } else {
            child
        };
        leaf.repo.write(
            "template.toml",
            &format!(
                "{}\n[questions.added]\ntype = 'string'\ndefault = 'new answer'\n",
                leaf.repo.read("template.toml")
            ),
        );
        leaf.repo.commit_all("ask a new question");
        let before = world.project.working_state();
        let previous = world.project.rev_parse("refs/tpl/stable");
        let json = tpl(
            &world.project,
            &["--json", "switch", &leaf.source(), "--defaults"],
        )
        .success()
        .json();
        assert_eq!(json["inherited"], true);
        assert_eq!(json["reusedAnswers"], serde_json::json!(["name"]));
        assert_eq!(world.project.rev_parse("refs/tpl/stable^"), previous);
        assert!(!world.project.has_ref("refs/tpl/child"));
        let config = Config::load(&world.project.path).unwrap();
        assert_eq!(config.template.root.as_deref(), Some("template"));
        assert_eq!(config.template.id.as_deref(), Some("stable"));
        assert_eq!(config.answers["name"], Value::String("personal".into()));
        assert_eq!(config.answers["obsolete"], Value::Bool(true));
        assert_eq!(config.answers["added"], Value::String("new answer".into()));
        assert!(!std::path::Path::new(&config.template.source).is_absolute());
        assert_eq!(
            world.project.git(&["show", "refs/tpl/stable:name"]),
            "personal"
        );
        let after = world.project.working_state();
        assert_eq!(before.head, after.head);
        assert_eq!(before.index, after.index);
        let rendered_files = |state: &common::WorkingState| {
            state
                .worktree
                .lines()
                .filter(|line| !line.starts_with(".config/git.tpl.toml "))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        assert_eq!(rendered_files(&before), rendered_files(&after));
        assert_eq!(world.project.read("name"), "personal");
    }
}

#[test]
fn unrelated_switches_rebuild_config_reuse_exact_names_and_merge_using_the_old_base() {
    let world = World::with_template(
        BASE,
        &[
            ("name.jinja", "{{ name }}"),
            ("old", "old"),
            ("custom", "original"),
        ],
    );
    world.init(&["--answer", "name=personal"]).success();
    world.project.write("custom", "project customization");
    world.project.commit_all("customize project");
    let mut config = Config::load(&world.project.path).unwrap();
    config.template.root = Some("template".into());
    config.answers.insert("obsolete".into(), Value::Bool(true));
    config.save(&world.project.path).unwrap();
    world.project.commit_all("record old settings");
    let replacement = target(
        "name = 'replacement'\n[questions.name]\ntype = 'string'\ndefault = 'different'\n[questions.renamed]\ntype = 'string'\ndefault = 'fresh'\n",
        &[
            ("name.jinja", "{{ name }}"),
            ("new.jinja", "{{ renamed }}"),
            ("custom", "original"),
        ],
    );
    let previous = world.project.rev_parse(&world.ref_name());
    let json = tpl(
        &world.project,
        &[
            "--json",
            "switch",
            &replacement.source(),
            "--defaults",
            "--merge",
        ],
    )
    .success()
    .json();
    assert_eq!(json["switch"]["inherited"], false);
    assert_eq!(json["switch"]["id"], "template");
    assert_eq!(json["merge"]["result"], "merged");
    assert_eq!(
        world.project.rev_parse(&format!("{}^", world.ref_name())),
        previous
    );
    assert_eq!(world.project.read("name"), "personal");
    assert_eq!(world.project.read("new"), "fresh");
    assert_eq!(world.project.read("custom"), "project customization");
    assert!(!world.project.exists("old"));
    let config = Config::load(&world.project.path).unwrap();
    assert!(config.template.root.is_none());
    assert!(!config.answers.contains_key("obsolete"));
    assert_eq!(world.project.git(&["diff", "--cached", "--name-only"]), "");
    assert_eq!(
        world.project.git(&["diff", "--name-only"]),
        ".config/git.tpl.toml"
    );
    tpl(&world.project, &["update", "--defaults"])
        .success()
        .says("Already up to date");
    // Switching back is another append, not a reset to the old source's rendering.
    let tip = world.project.rev_parse(&world.ref_name());
    tpl(
        &world.project,
        &["switch", &world.template.source(), "--defaults"],
    )
    .success();
    assert_eq!(
        world.project.rev_parse(&format!("{}^", world.ref_name())),
        tip
    );
}

#[test]
fn incompatible_candidates_fall_back_but_explicit_invalid_answers_fail() {
    let cases = [
        (
            "type = 'string'\ndefault = 'true'",
            "type = 'boolean'\ndefault = false",
            Value::Bool(false),
        ),
        (
            "type = 'choice'\nchoices = ['old']\ndefault = 'old'",
            "type = 'choice'\nchoices = ['new']\ndefault = 'new'",
            Value::String("new".into()),
        ),
        (
            "type = 'string'\ndefault = 'UPPER'",
            "type = 'string'\npattern = '^[a-z]+$'\ndefault = 'lower'",
            Value::String("lower".into()),
        ),
        (
            "type = 'multi_choice'\nchoices = ['a', 'b']\ndefault = ['a', 'b']",
            "type = 'multi_choice'\nchoices = ['a', 'b']\nmax_choices = 1\ndefault = ['b']",
            Value::Array(vec![Value::String("b".into())]),
        ),
        (
            "type = 'list'\ndefault = ['BAD']",
            "type = 'list'\nitem_pattern = '^[a-z]+$'\ndefault = ['good']",
            Value::Array(vec![Value::String("good".into())]),
        ),
    ];
    for (old, new, expected) in cases {
        let world = World::with_template(
            &format!("name = 'old'\n[questions.answer]\n{old}\n"),
            &[("out.jinja", "{{ answer }}")],
        );
        world.init(&[]).success();
        let replacement = target(
            &format!("name = 'new'\n[questions.answer]\n{new}\n"),
            &[("out.jinja", "{{ answer }}")],
        );
        let answers = world.project.read(".config/git.tpl.toml");
        let answer_file = world.project.path.join("recorded.toml");
        let old_config = Config::load(&world.project.path).unwrap();
        world.project.write(
            "recorded.toml",
            &toml::to_string(&old_config.answers).unwrap(),
        );
        let before = world.project.working_state();
        let tip = world.project.rev_parse(&world.ref_name());
        tpl(
            &world.project,
            &[
                "switch",
                &replacement.source(),
                "--defaults",
                "--answers-from",
                answer_file.to_str().unwrap(),
            ],
        )
        .failure();
        assert_eq!(world.project.working_state(), before);
        assert_eq!(world.project.rev_parse(&world.ref_name()), tip);
        assert_eq!(world.project.read(".config/git.tpl.toml"), answers);
        let json = tpl(
            &world.project,
            &["--json", "switch", &replacement.source(), "--defaults"],
        )
        .success()
        .json();
        assert_eq!(json["reusedAnswers"], serde_json::json!([]));
        assert_eq!(
            Config::load(&world.project.path).unwrap().answers["answer"],
            expected
        );
    }
}

#[test]
fn runtime_choices_use_reused_dependencies_and_explicit_overrides_win() {
    let world = World::with_template(
        "name = 'old'\n[questions.region]\ntype = 'string'\ndefault = 'eu'\n[questions.pick]\ntype = 'string'\ndefault = 'eu'\n",
        &[("out.jinja", "{{ pick }}")],
    );
    world.init(&[]).success();
    let replacement = target(
        "name = 'new'\n[questions.region]\ntype = 'string'\ndefault = 'us'\n[questions.pick]\ntype = 'choice'\nchoices_from = 'catalogue'\ndefault = '{{ region }}'\n[computed]\ncatalogue = '{{ [region] }}'\n",
        &[("out.jinja", "{{ pick }}")],
    );
    let json = tpl(
        &world.project,
        &[
            "--json",
            "switch",
            &replacement.source(),
            "--defaults",
            "--answer",
            "region=asia",
            "--strict-answers",
        ],
    )
    .success()
    .json();
    assert_eq!(json["reusedAnswers"], serde_json::json!([]));
    let config = Config::load(&world.project.path).unwrap();
    assert_eq!(config.answers["pick"], Value::String("asia".into()));
}

#[test]
fn same_output_switch_records_target_provenance_and_sets_its_migration_baseline() {
    let world = World::with_template("name = 'old'", &[("out", "identical")]);
    world.init(&[]).success();
    let replacement = target("name = 'new'", &[("out", "identical")]);
    replacement.repo.write(
        "migrations/old.toml",
        "message = '{{ missing | int }}'\n[[moves]]\nfrom = 'absent'\nto = 'elsewhere'\n",
    );
    replacement
        .repo
        .commit_all("historical migration must not run during switch");
    let previous = world.project.rev_parse(&world.ref_name());
    tpl(
        &world.project,
        &[
            "switch",
            &replacement.source(),
            "--defaults",
            "--ref",
            "main",
        ],
    )
    .success();
    let tip = world.project.rev_parse(&world.ref_name());
    assert_ne!(tip, previous);
    assert_eq!(world.project.rev_parse(&format!("{tip}^")), previous);
    assert_eq!(
        world.project.rev_parse(&format!("{tip}^{{tree}}")),
        world.project.rev_parse(&format!("{previous}^{{tree}}"))
    );
    let config = Config::load(&world.project.path).unwrap();
    assert_eq!(config.template.r#ref.as_deref(), Some("main"));
    let message = world
        .project
        .git(&["log", "-1", "--format=%B", &world.ref_name()]);
    assert!(message.contains(&format!("Template-Source: {}", config.template.source)));
    tpl(&world.project, &["update", "--defaults"])
        .success()
        .says("Already up to date");
}

#[test]
fn required_missing_answers_and_missing_history_fail_without_changing_state() {
    for missing_history in [false, true] {
        let world = World::with_template(BASE, &[("out", "base")]);
        world.init(&[]).success();
        let replacement = target(
            "name = 'new'\n[questions.required]\ntype = 'string'",
            &[("out", "new")],
        );
        if missing_history {
            world.project.git(&["update-ref", "-d", &world.ref_name()]);
        }
        let before = world.project.working_state();
        let output = tpl(
            &world.project,
            &["--json", "switch", &replacement.source(), "--defaults"],
        )
        .failure();
        assert_eq!(
            output.error_code(),
            if missing_history {
                "tpl::ops::no_rendered_ref"
            } else {
                "tpl::eval::unanswered"
            }
        );
        assert_eq!(world.project.working_state(), before);
    }
}

#[test]
fn only_missing_questions_are_prompted_and_concurrent_changes_are_preserved() {
    use tpl::eval::{EvalError, Prompter};
    use tpl::git::LibGit2;
    use tpl::ops::{Answering, Trust};
    use tpl::template::{Choice, Question};
    struct DuringPrompt<F>(F, Vec<String>);
    impl<F: FnMut()> Prompter for DuringPrompt<F> {
        fn ask(
            &mut self,
            name: &str,
            _: &Question,
            default: Option<&Value>,
            _: Option<&Value>,
            _: Option<&[Choice]>,
        ) -> Result<Value, EvalError> {
            self.1.push(name.into());
            (self.0)();
            Ok(default.unwrap().clone())
        }
    }
    for action in ["none", "config", "ref", "delete"] {
        let world = World::with_template(BASE, &[("out.jinja", "{{ name }}")]);
        world.init(&[]).success();
        let replacement = target(
            &format!("{BASE}\n[questions.added]\ntype = 'string'\ndefault = 'new'"),
            &[("out.jinja", "{{ name }} {{ added }}")],
        );
        let before = world.project.working_state();
        let tip = world.project.rev_parse(&world.ref_name());
        let mut prompter = DuringPrompt(
            || match action {
                "config" => {
                    world.project.write(
                        ".config/git.tpl.toml",
                        &format!(
                            "{}\n# concurrent hand edit\n[answers.extra]\nx = true\n",
                            world.project.read(".config/git.tpl.toml")
                        ),
                    );
                }
                "ref" => {
                    world
                        .project
                        .git(&["update-ref", &world.ref_name(), "HEAD"]);
                }
                "delete" => {
                    world.project.git(&["update-ref", "-d", &world.ref_name()]);
                }
                _ => (),
            },
            Vec::new(),
        );
        let backend = LibGit2::discover(&world.project.path).unwrap();
        let result = tpl::ops::switch(
            &backend,
            &world.project.path,
            &replacement.source(),
            None,
            Default::default(),
            false,
            false,
            &Default::default(),
            &tpl::git::cache::Access::new(None, false),
            Answering::Interactive(&mut prompter),
            Trust::always(),
        );
        assert_eq!(prompter.1, vec!["added"]);
        if action == "none" {
            assert!(result.is_ok());
        } else {
            assert!(result.is_err());
        }
        let after = world.project.working_state();
        assert_eq!(after.head, before.head);
        assert_eq!(after.index, before.index);
        if action == "config" {
            assert_eq!(world.project.rev_parse(&world.ref_name()), tip);
            assert!(
                world
                    .project
                    .read(".config/git.tpl.toml")
                    .contains("concurrent hand edit")
            );
        } else if action != "none" {
            assert_eq!(after, before);
        }
    }
}

#[test]
fn switch_merge_refuses_dirty_state_and_reports_normal_conflicts() {
    let world = World::with_template(BASE, &[("out", "base\n")]);
    world.init(&[]).success();
    let replacement = target(BASE, &[("out", "target\n")]);
    world.project.write("out", "local\n");
    let before = world.project.working_state();
    let tip = world.project.rev_parse(&world.ref_name());
    assert_eq!(
        tpl(
            &world.project,
            &[
                "--json",
                "switch",
                &replacement.source(),
                "--defaults",
                "--merge"
            ]
        )
        .failure()
        .error_code(),
        "tpl::git::dirty_worktree"
    );
    assert_eq!(world.project.working_state(), before);
    assert_eq!(world.project.rev_parse(&world.ref_name()), tip);
    world.project.commit_all("local customization");
    let json = tpl(
        &world.project,
        &[
            "--json",
            "switch",
            &replacement.source(),
            "--defaults",
            "--merge",
        ],
    )
    .success()
    .json();
    assert_eq!(json["merge"]["result"], "conflicted");
    assert!(world.project.exists(".git/MERGE_HEAD"));
    tpl(&world.project, &["merge", "--abort"]).success();
    assert_eq!(world.project.read("out"), "local\n");
}

#[test]
fn inherited_root_overrides_are_applied_before_selecting_the_target_files() {
    let world = World::with_template(BASE, &[("out", "base")]);
    world.init(&[]).success();
    let mut config = Config::load(&world.project.path).unwrap();
    config.template.root = Some("alternate".into());
    config.save(&world.project.path).unwrap();
    let child = Template::extending("child", &world.template, "v1", "", &[]);
    child.repo.write("alternate/child", "selected override");
    child.repo.write("template/child", "wrong default root");
    child.repo.commit_all("add both roots");
    tpl(&world.project, &["switch", &child.source(), "--defaults"]).success();
    assert_eq!(
        world.project.git(&["show", "refs/tpl/template:child"]),
        "selected override"
    );
}

#[test]
fn skipped_answers_are_retained_only_for_inherited_configuration() {
    for inherited in [false, true] {
        let world = World::with_template(BASE, &[("out.jinja", "{{ name }}")]);
        world.init(&[]).success();
        let extra = "[questions.name]\ntype = 'string'\nwhen = '{{ false }}'\n";
        let replacement = if inherited {
            Template::extending(
                "child",
                &world.template,
                "v1",
                extra,
                &[("out.jinja", "skipped")],
            )
        } else {
            target(&format!("name = 'new'\n{extra}"), &[("out", "skipped")])
        };
        let json = tpl(
            &world.project,
            &[
                "--json",
                "switch",
                &replacement.source(),
                "--defaults",
                "--strict-answers",
            ],
        )
        .success()
        .json();
        assert_eq!(json["reusedAnswers"], serde_json::json!([]));
        assert_eq!(
            Config::load(&world.project.path)
                .unwrap()
                .answers
                .contains_key("name"),
            inherited
        );
    }
}

#[test]
fn switches_replace_composed_locks_with_the_target_child_revisions() {
    let child = target(
        "name = 'module'\n[questions.module_name]\ntype = 'string'\n[questions.module_path]\ntype = 'string'\n",
        &[("{{ module_path }}/child.jinja", "{{ module_name }}")],
    );
    let manifest = format!(
        "name = 'parent'\n[questions.modules]\ntype = 'list'\ndefault = ['one']\n[[compose]]\nsource = '{}'\nref = 'main'\nfrom = 'modules'\n",
        child.declared_source()
    );
    let world = World::with_template(&manifest, &[("out", "parent")]);
    world.init(&[]).success();
    let old = Config::load(&world.project.path).unwrap().composed;
    child.repo.write(
        "template/{{ module_path }}/child.jinja",
        "new {{ module_name }}",
    );
    child.repo.commit_all("advance child");
    let replacement = target(&manifest, &[("out", "new parent")]);
    tpl(
        &world.project,
        &["switch", &replacement.source(), "--defaults"],
    )
    .success();
    let config = Config::load(&world.project.path).unwrap();
    assert_ne!(config.composed, old);
    assert_eq!(config.composed[0].commit, child.repo.rev_parse("HEAD"));
    assert_eq!(
        world
            .project
            .git(&["show", "refs/tpl/template:modules/one/child"]),
        "new one"
    );
}
