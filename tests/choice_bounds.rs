//! Interactive bounds must let the user correct the current selection.

#![cfg(unix)]

mod common;

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use common::World;

#[test]
fn invalid_selection_counts_retry_in_place_and_can_be_corrected_or_cancelled() {
    for scenario in 0..5 {
        let cancel = scenario == 1;
        let catalogue = if scenario == 3 {
            "[computed]\noptions = \"{{ [{'value': 'a', 'label': 'First A', 'help': 'First help'}, {'value': 'a', 'label': 'Hidden duplicate'}, {'value': 'b', 'label': 'Second B'}] }}\"\n[questions.pick]\nchoices_from = 'options'\n"
        } else {
            "[questions.pick]\nchoices = [{value = 'a', label = 'First A', help = 'First help'}, {value = 'a', label = 'Hidden duplicate'}, {value = 'b', label = 'Second B'}]\n"
        };
        let manifest = if scenario >= 2 {
            format!(
                "name = 'distinct'\n{catalogue}type = 'multi_choice'\nprompt = 'Bounded selection'\nmin_choices = 2\nmax_choices = 2\ndefault = []\n"
            )
        } else {
            "name = 'bounds'\n[questions.pick]\ntype = 'multi_choice'\nprompt = 'Bounded selection'\nchoices = ['a', 'b']\nmin_choices = 1\nmax_choices = 1\ndefault = []\n".into()
        };
        let manifest = if scenario == 4 {
            manifest.replace("default = []", "default = ['b', 'a', 'b']")
        } else {
            manifest
        };
        let world = World::with_template(&manifest, &[("out.jinja", "{{ pick }}")]);
        let pair = portable_pty::native_pty_system()
            .openpty(portable_pty::PtySize {
                rows: 24,
                cols: 100,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command =
            portable_pty::CommandBuilder::new(assert_cmd::cargo::cargo_bin!("git-tpl"));
        command.cwd(&world.project.path);
        command.env_clear();
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
        let mut wait_for = |needle: &str| {
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
        };
        wait_for("Bounded selection");
        writer.write_all(b"\r").unwrap();
        writer.flush().unwrap();
        wait_for(if scenario == 4 {
            "\"ok\":true"
        } else if scenario >= 2 {
            "Please select at least 2 options"
        } else {
            "Please select an option"
        });
        if cancel {
            // Escape exercises cancellation without racing read_key's raw-mode setup:
            // Ctrl-C between reads becomes SIGINT and bypasses the JSON diagnostic.
            writer.write_all(b"\x1b").unwrap();
            writer.flush().unwrap();
            wait_for("tpl::eval::cancelled");
        } else if scenario != 4 {
            // With duplicate aliases, two rows must still select two distinct values.
            writer.write_all(b" \x1b[B \r").unwrap();
            writer.flush().unwrap();
            if scenario < 2 {
                wait_for("Please select only one option");
                writer.write_all(b" \r").unwrap();
                writer.flush().unwrap();
            }
            wait_for("\"ok\":true");
        }
        if scenario >= 2 {
            // The first two rows must be distinct, not two aliases of `a`.
            assert!(output.contains("\"pick\":[\"a\",\"b\"]"), "{output}");
            assert!(output.contains("First A"), "{output}");
            assert!(output.contains("First help"), "{output}");
            let widget = output.split("{\"answers\"").next().unwrap();
            assert!(!widget.contains("Hidden duplicate"), "{output}");
        }
        let status = child.wait().unwrap();
        assert_eq!(status.success(), !cancel);
    }
}
