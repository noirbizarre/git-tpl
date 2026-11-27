//! Primitive constraints apply before rendering or publishing, regardless of answer provenance.

mod common;

use common::{World, tpl};
use tpl::template::Manifest;

#[test]
fn multi_choice_bounds_count_distinct_values_without_rewriting_any_answer_source() {
    for catalogue in [
        "[questions.pick]\nchoices = ['a', 'b', 'b']\n",
        "[computed]\noptions = \"{{ ['a', 'b', 'b'] }}\"\n[questions.pick]\nchoices_from = 'options'\n",
    ] {
        let manifest = format!(
            "name = 'distinct'\n{catalogue}type = 'multi_choice'\nmin_choices = 2\nmax_choices = 2\ndefault = ['b', 'a', 'b']\n"
        );
        let world = World::with_template(&manifest, &[("out.jinja", "{{ pick | join(',') }}")]);
        let expected = serde_json::json!(["b", "a", "b"]);
        for extra in [vec![], vec!["--answer", "pick=b,a,b"]] {
            let source = world.template.source();
            let mut args = vec!["--json", "context", &source, "--defaults"];
            args.extend(extra);
            assert_eq!(
                tpl(&world.project, &args).success().json()["answers"]["pick"],
                expected
            );
        }
        let path = world.dir.path().join("answers.toml");
        std::fs::write(&path, "pick = ['b', 'a', 'b']\n").unwrap();
        let json = tpl(
            &world.project,
            &[
                "--json",
                "context",
                &world.template.source(),
                "--defaults",
                "--answers-from",
                path.to_str().unwrap(),
            ],
        )
        .success()
        .json();
        assert_eq!(json["answers"]["pick"], expected);

        world.init(&[]).success();
        assert_eq!(world.project.read("out"), "b,a,b");
        let before = world.project.working_state();
        let tip = world.project.rev_parse(&world.ref_name());
        tpl(&world.project, &["update", "--defaults"]).success();
        assert_eq!(world.project.working_state(), before);
        assert_eq!(world.project.rev_parse(&world.ref_name()), tip);

        // Repetitions cannot satisfy the minimum, even when every entry is offered.
        assert_eq!(
            tpl(
                &world.project,
                &["--json", "update", "--defaults", "--answer", "pick=a,a"]
            )
            .failure()
            .error_code(),
            "tpl::eval::choice_count"
        );
        assert_eq!(world.project.working_state(), before);
        assert_eq!(world.project.rev_parse(&world.ref_name()), tip);
    }
}

#[test]
fn ordinary_list_bounds_still_count_duplicate_entries() {
    let world = World::with_template(
        "name = 'list'\n[questions.items]\ntype = 'list'\nmin_items = 2\nmax_items = 2\ndefault = ['a', 'a']\n",
        &[("out.jinja", "{{ items | join(',') }}")],
    );
    world.init(&[]).success();
    assert_eq!(world.project.read("out"), "a,a");
}

#[test]
fn incompatible_inverted_and_nonfinite_constraint_declarations_are_rejected() {
    for declaration in [
        "type = 'string'\nmin = 1",
        "type = 'boolean'\nmax = 2",
        "type = 'integer'\nmin = 1.0",
        "type = 'integer'\nmax = 2.5",
        "type = 'integer'\nmin = 9007199254740993\nmax = 9007199254740992",
        "type = 'float'\nmin = 2.5\nmax = 2",
        "type = 'float'\nmin = nan",
        "type = 'float'\nmax = inf",
        "type = 'float'\nmin = -inf",
        "type = 'integer'\nmin_length = 1",
        "type = 'list'\nmax_length = 1",
        "type = 'string'\nmin_length = 3\nmax_length = 2",
        "type = 'string'\nmax_length = 0",
        "type = 'list'\nmin_items = 3\nmax_items = 2",
        "type = 'list'\nitem_min_length = 3\nitem_max_length = 2",
        "type = 'multi_choice'\nchoices = ['a']\nmin_items = 1",
        "type = 'string'\nitem_min_length = 1",
        "type = 'boolean'\nitem_max_length = 1",
        "type = 'string'\nmax_items = 1",
        "type = 'string'\nmin_length = -1",
        "type = 'string'\nmax_length = 1.5",
        "type = 'list'\nmin_items = -1",
        "type = 'list'\nmax_items = -1",
        "type = 'list'\nitem_min_length = -1",
        "type = 'list'\nitem_max_length = -1",
    ] {
        let manifest = format!("name = 'invalid'\n[questions.input]\n{declaration}\n");
        assert!(
            Manifest::parse(&manifest, "template.toml").is_err(),
            "{declaration}"
        );
    }
}

#[test]
fn introspection_exposes_numeric_string_and_list_bounds() {
    let world = World::with_template(
        "name = 'schema'\n[questions.workers]\ntype = 'integer'\nmin = 1\nmax = 9007199254740993\n[questions.timeout]\ntype = 'float'\nmin = 0.1\nmax = 60\n[questions.name]\ntype = 'string'\nmin_length = 2\nmax_length = 64\n[questions.modules]\ntype = 'list'\nmin_items = 1\nmax_items = 10\nitem_min_length = 2\nitem_max_length = 32\n",
        &[("out", "")],
    );
    let json = tpl(
        &world.project,
        &["--json", "questions", &world.template.source()],
    )
    .success()
    .json();
    let questions = json["questions"].as_array().unwrap();
    let question = |name| questions.iter().find(|q| q["name"] == name).unwrap();
    assert_eq!(question("workers")["min"], 1);
    assert_eq!(question("workers")["max"], 9007199254740993_i64);
    assert_eq!(question("timeout")["type"], "float");
    assert_eq!(question("timeout")["min"], 0.1);
    assert_eq!(question("timeout")["max"], 60);
    assert_eq!(question("name")["minLength"], 2);
    assert_eq!(question("name")["maxLength"], 64);
    for (key, expected) in [
        ("minItems", 1),
        ("maxItems", 10),
        ("itemMinLength", 2),
        ("itemMaxLength", 32),
    ] {
        assert_eq!(question("modules")[key], expected);
    }
    assert!(question("modules")["minChoices"].is_null());
    assert!(question("name")["min"].is_null());
}

#[test]
fn cli_and_all_answer_file_formats_enforce_the_same_primitive_constraints() {
    for (kind, bounds, valid, invalid, code) in [
        ("integer", "min = 1\nmax = 2", "2", "3", "numeric_bound"),
        (
            "float",
            "min = 0.5\nmax = 2.5",
            "2.5",
            "2.6",
            "numeric_bound",
        ),
        (
            "string",
            "min_length = 2\nmax_length = 2",
            "\"éé\"",
            "\"é\"",
            "string_length",
        ),
        (
            "list",
            "min_items = 1\nmax_items = 1",
            "[\"ab\"]",
            "[]",
            "item_count",
        ),
        (
            "list",
            "item_min_length = 2\nitem_max_length = 2",
            "[\"ab\",\"éé\"]",
            "[\"ab\",\"é\"]",
            "invalid_list",
        ),
    ] {
        let world = World::with_template(
            &format!("name = 'sources'\n[questions.input]\ntype = '{kind}'\n{bounds}\n"),
            &[("out.jinja", "{{ input }}")],
        );
        for (answer, accepted) in [(valid, true), (invalid, false)] {
            let cli = if kind == "string" {
                answer.trim_matches('"')
            } else {
                answer
            };
            let result = tpl(
                &world.project,
                &[
                    "--json",
                    "context",
                    &world.template.source(),
                    "--defaults",
                    "--answer",
                    &format!("input={cli}"),
                ],
            );
            if accepted {
                result.success();
            } else {
                assert_eq!(result.failure().error_code(), format!("tpl::eval::{code}"));
            }
            for (extension, body) in [
                ("toml", format!("input = {answer}")),
                ("json", format!("{{\"input\":{answer}}}")),
                ("yaml", format!("input: {answer}")),
            ] {
                let path = world.dir.path().join(format!("answers.{extension}"));
                std::fs::write(&path, body).unwrap();
                let result = tpl(
                    &world.project,
                    &[
                        "--json",
                        "context",
                        &world.template.source(),
                        "--defaults",
                        "--answers-from",
                        path.to_str().unwrap(),
                    ],
                );
                if accepted {
                    result.success();
                } else {
                    assert_eq!(result.failure().error_code(), format!("tpl::eval::{code}"));
                }
            }
        }
    }
}

#[test]
fn nonfinite_cli_and_typed_float_answers_fail_with_question_context() {
    let world = World::with_template(
        "name = 'finite'\n[questions.timeout]\ntype = 'float'\n",
        &[("out.jinja", "{{ timeout }}")],
    );
    for answer in ["NaN", "inf", "-inf", "1e999"] {
        let error = tpl(
            &world.project,
            &[
                "--json",
                "context",
                &world.template.source(),
                "--defaults",
                "--answer",
                &format!("timeout={answer}"),
            ],
        )
        .failure();
        assert_eq!(error.error_code(), "tpl::eval::numeric_bound");
        error.says("timeout").says("finite");
    }
    for (extension, body) in [("toml", "timeout = nan"), ("yaml", "timeout: .inf")] {
        let path = world.dir.path().join(format!("answers.{extension}"));
        std::fs::write(&path, body).unwrap();
        assert_eq!(
            tpl(
                &world.project,
                &[
                    "--json",
                    "context",
                    &world.template.source(),
                    "--defaults",
                    "--answers-from",
                    path.to_str().unwrap()
                ]
            )
            .failure()
            .error_code(),
            "tpl::eval::numeric_bound"
        );
    }
}

#[test]
fn computed_numeric_defaults_and_float_persistence_survive_an_unchanged_update() {
    for answer in ["2", "2.5", "-0.0"] {
        let world = World::with_template(
            &format!(
                "name = 'floats'\n[computed]\nvalue = '{{{{ {answer} }}}}'\n[questions.timeout]\ntype = 'float'\ndefault = '{{{{ value }}}}'\nmin = -1\nmax = 3\n"
            ),
            &[("out.jinja", "{{ timeout }}")],
        );
        world.init(&[]).success();
        let config = world.project.read(".config/git.tpl.toml");
        let before = world.project.working_state();
        let tip = world.project.rev_parse(&world.ref_name());
        tpl(&world.project, &["update", "--defaults"]).success();
        assert_eq!(world.project.read(".config/git.tpl.toml"), config);
        assert_eq!(world.project.working_state(), before);
        assert_eq!(world.project.rev_parse(&world.ref_name()), tip);
    }
}

#[test]
fn narrower_primitive_constraints_reject_recorded_answers_without_changing_repository_state() {
    for (kind, default, constraint, code) in [
        ("integer", "2", "max = 1", "numeric_bound"),
        ("float", "2.5", "max = 2", "numeric_bound"),
        ("string", "'ab'", "max_length = 1", "string_length"),
        ("list", "['ab', 'cd']", "max_items = 1", "item_count"),
        (
            "list",
            "['ab', 'cd']",
            "item_max_length = 1",
            "invalid_list",
        ),
    ] {
        let manifest =
            format!("name = 'narrow'\n[questions.input]\ntype = '{kind}'\ndefault = {default}\n");
        let world = World::with_template(&manifest, &[("out.jinja", "{{ input }}")]);
        world.init(&[]).success();
        world
            .template
            .repo
            .write("template.toml", &format!("{manifest}{constraint}\n"))
            .commit_all("narrow constraints");
        let before = world.project.working_state();
        let tip = world.project.rev_parse(&world.ref_name());
        assert_eq!(
            tpl(&world.project, &["--json", "update", "--defaults"])
                .failure()
                .error_code(),
            format!("tpl::eval::{code}")
        );
        assert_eq!(world.project.working_state(), before);
        assert_eq!(world.project.rev_parse(&world.ref_name()), tip);
    }
}
