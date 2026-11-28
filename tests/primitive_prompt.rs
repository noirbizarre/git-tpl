//! Real terminals exercise retry behavior after defaults and individual list entries are validated.

#![cfg(unix)]

mod common;

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use common::World;

fn questionnaire(declaration: &str, steps: &[(&str, &[u8])], success: bool) {
    let world = World::with_template(
        &format!(
            "name = 'retry'\n[questions.input]\nprompt = 'Constrained input'\n{declaration}\n"
        ),
        &[("out.jinja", "{{ input }}")],
    );
    let pair = portable_pty::native_pty_system()
        .openpty(portable_pty::PtySize {
            rows: 24,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let original_termios = format!("{:?}", pair.master.get_termios());
    let mut command = portable_pty::CommandBuilder::new(assert_cmd::cargo::cargo_bin!("git-tpl"));
    command.cwd(&world.project.path);
    command.env_clear();
    // The isolated environment must still route child coverage to llvm-cov.
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    command.env("PATH", std::env::var_os("PATH").unwrap());
    command.env("HOME", world.project.config_home());
    command.env("XDG_CONFIG_HOME", world.project.config_home());
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    command.env(
        "GIT_CONFIG_GLOBAL",
        world.project.config_home().join("absent.gitconfig"),
    );
    command.env("TERM", "xterm");
    command.args(["--json", "context", &world.template.source()]);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0; 4096];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0
                || sender
                    .send(String::from_utf8_lossy(&buffer[..count]).into_owned())
                    .is_err()
            {
                break;
            }
        }
    });
    let mut output = String::new();
    for (needle, input) in steps
        .iter()
        .copied()
        .chain(success.then_some(("\"ok\":true", &b""[..])))
    {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !output.contains(needle) {
            match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(chunk) => output.push_str(&chunk),
                Err(error) => {
                    let _ = child.kill();
                    panic!("waiting for {needle:?}: {error}; output: {output}");
                }
            }
        }
        // Consume through the match so a redraw of an earlier prompt cannot satisfy the next step.
        let consumed = output.find(needle).unwrap() + needle.len();
        output.drain(..consumed);
        if input == b"\x03" {
            // Send the interrupt to the widget's read rather than race a
            // cooked-mode redraw and signal the process group ourselves.
            let deadline = Instant::now() + Duration::from_secs(5);
            while format!("{:?}", pair.master.get_termios()) == original_termios {
                assert!(Instant::now() < deadline, "input never entered raw mode");
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        writer.write_all(input).unwrap();
        writer.flush().unwrap();
    }
    assert_eq!(child.wait().unwrap().success(), success);
}

#[test]
fn numeric_and_string_constraints_retry_invalid_input_and_invalid_placeholders() {
    for (declaration, reason, correction) in [
        (
            "type = 'integer'\nmin = 2\ndefault = 1",
            "`min` is 2",
            &b"2\r"[..],
        ),
        (
            "type = 'float'\nmax = 2.5\ndefault = 3.0",
            "`max` is 2.5",
            &b"2.5\r"[..],
        ),
        (
            "type = 'string'\nmin_length = 2\ndefault = 'a'",
            "at least 2 characters",
            &b"ab\r"[..],
        ),
    ] {
        questionnaire(
            declaration,
            &[
                ("Constrained input", b"\r"),
                (reason, b""),
                ("Constrained input", correction),
            ],
            true,
        );
    }
    questionnaire(
        "type = 'float'",
        &[
            ("Constrained input", b"NaN\r"),
            ("must be finite", b""),
            ("Constrained input", b"2\r"),
        ],
        true,
    );
}

#[test]
fn list_minimum_and_item_length_retry_without_discarding_accepted_entries() {
    questionnaire(
        "type = 'list'\nmin_items = 2\nmax_items = 2\nitem_min_length = 2",
        &[
            ("Constrained input #1", b"\r"),
            ("at least 2 items", b""),
            ("Constrained input #1", b"ab\r"),
            ("Constrained input #2", b"x\r"),
            ("item 2 of `input` is invalid", b""),
            ("Constrained input #2", b"cd\r"),
        ],
        true,
    );
}

#[test]
fn lists_at_capacity_allow_explicit_removal_of_remaining_defaults() {
    questionnaire(
        "type = 'list'\nmax_items = 1\ndefault = ['ab', 'cd']",
        &[
            ("Constrained input #1", b"\r"),
            ("Constrained input #2", b"\r"),
            ("allows at most 1 items", b""),
            ("Constrained input #2", b"-\r"),
        ],
        true,
    );
    questionnaire("type = 'list'\nmax_items = 0", &[], true);
}

#[test]
fn constraint_retry_can_be_interrupted_without_restarting_the_questionnaire() {
    questionnaire(
        "type = 'integer'\nmin = 2",
        &[
            ("Constrained input", b"1\r"),
            ("`min` is 2", b""),
            // Dialoguer text uses Ctrl-C; its current signal-based exit is
            // characterized separately in prompts.rs and the integration report.
            ("Constrained input", b"\x03"),
        ],
        false,
    );
}
