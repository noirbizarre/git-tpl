//! Help exits during argument parsing, before normal command presentation.

use assert_cmd::Command;

fn help(args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut command = Command::cargo_bin("git-tpl").expect("built binary");
    command.current_dir(dir.path()).args(args);
    // Child-local settings avoid both ambient terminal preferences and races
    // between parallel tests exercising contradictory environment policies.
    for key in ["NO_COLOR", "FORCE_COLOR", "CLICOLOR_FORCE", "TERM"] {
        command.env_remove(key);
    }
    command.env("TERM", "dumb");
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().expect("run git-tpl")
}

fn has_color(bytes: &[u8]) -> bool {
    bytes.windows(2).any(|pair| pair == b"\x1b[")
}

#[test]
fn help_works_without_a_repository_or_manual_pages() {
    for args in [vec!["help"], vec!["--json", "help"], vec!["--help"]] {
        let out = help(&args, &[]);
        assert!(out.status.success(), "{out:?}");
        assert!(out.stderr.is_empty(), "{out:?}");
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(text.contains("Usage: git tpl"), "{text}");
        assert!(text.contains("Commands:"), "{text}");
    }
    let out = help(&["help", "update"], &[]);
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("Usage: git tpl update"));
}

#[test]
fn help_uses_the_shared_color_environment_precedence() {
    for (env, colored) in [
        (vec![], false),
        (vec![("NO_COLOR", "")], false),
        (vec![("FORCE_COLOR", "1")], true),
        (vec![("CLICOLOR_FORCE", "1")], true),
        (vec![("FORCE_COLOR", "0")], false),
        (vec![("NO_COLOR", "1"), ("FORCE_COLOR", "1")], true),
    ] {
        let out = help(&["help"], &env);
        assert!(out.status.success(), "{out:?}");
        assert_eq!(has_color(&out.stdout), colored, "{env:?}: {out:?}");
    }
}

#[test]
fn explicit_color_overrides_the_environment_before_help_exits() {
    for args in [
        vec!["--color", "always", "help", "update"],
        vec!["--color=always", "--help"],
        vec!["--help", "--color", "always"],
        vec!["update", "--color", "always", "-h"],
        vec!["update", "-h", "--color=always"],
    ] {
        let out = help(&args, &[("NO_COLOR", "1")]);
        assert!(out.status.success(), "{args:?}: {out:?}");
        assert!(has_color(&out.stdout), "{args:?}: {out:?}");
    }
    let out = help(&["--color=never", "help"], &[("FORCE_COLOR", "1")]);
    assert!(out.status.success(), "{out:?}");
    assert!(!has_color(&out.stdout), "{out:?}");
}

#[test]
fn argument_errors_use_the_same_color_policy_and_exit_status() {
    for (args, env, colored) in [
        (vec!["unknown"], vec![("FORCE_COLOR", "1")], true),
        (vec!["--color=always", "unknown"], vec![], true),
        (
            vec!["--color=never", "unknown"],
            vec![("FORCE_COLOR", "1")],
            false,
        ),
        (vec!["--json", "unknown"], vec![("NO_COLOR", "1")], false),
    ] {
        let out = help(&args, &env);
        assert_eq!(out.status.code(), Some(2), "{out:?}");
        assert!(out.stdout.is_empty(), "{out:?}");
        assert_eq!(has_color(&out.stderr), colored, "{out:?}");
    }
}

#[test]
fn positional_tokens_and_option_values_do_not_select_colors() {
    for args in [
        vec!["--", "--color=always"],
        vec!["help", "--color=always"],
        vec!["render", "--answers-from=--color=always", "--help"],
    ] {
        let out = help(&args, &[]);
        assert!(!has_color(&out.stdout), "{args:?}: {out:?}");
        assert!(!has_color(&out.stderr), "{args:?}: {out:?}");
    }
}
