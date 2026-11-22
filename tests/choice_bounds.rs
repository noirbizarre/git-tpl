//! Interactive bounds must let the user correct the current selection.

#![cfg(unix)]

mod common;

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use common::World;

#[test]
fn invalid_selection_counts_retry_in_place_and_can_be_corrected_or_cancelled() {
    for cancel in [false, true] {
        let world = World::with_template(
            "name = 'bounds'\n[questions.pick]\ntype = 'multi_choice'\nprompt = 'Bounded selection'\nchoices = ['a', 'b']\nmin_choices = 1\nmax_choices = 1\ndefault = []\n",
            &[("out.jinja", "{{ pick }}")],
        );
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
        wait_for("Please select an option");
        if cancel {
            // Escape exercises cancellation without racing read_key's raw-mode setup:
            // Ctrl-C between reads becomes SIGINT and bypasses the JSON diagnostic.
            writer.write_all(b"\x1b").unwrap();
            writer.flush().unwrap();
            wait_for("tpl::eval::cancelled");
        } else {
            // Select both, confirm, then deselect the second without restarting.
            writer.write_all(b" \x1b[B \r").unwrap();
            writer.flush().unwrap();
            wait_for("Please select only one option");
            writer.write_all(b" \r").unwrap();
            writer.flush().unwrap();
            wait_for("\"ok\":true");
        }
        let status = child.wait().unwrap();
        assert_eq!(status.success(), !cancel);
    }
}
