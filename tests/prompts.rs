//! Real-terminal characterization of the choisir integration.
#![cfg(unix)]

mod common;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use common::{World, tpl};

/// Keep stderr on a terminal while capturing stdout independently: merging
/// them would hide exactly the mailbox/JSON contamination these tests prevent.
struct Terminal {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    chunks: Receiver<Vec<u8>>,
    output: Vec<u8>,
    consumed: usize,
    stdout: PathBuf,
    termios: String,
}

impl Terminal {
    fn start(world: &World, args: &[&str]) -> Self {
        let pair = portable_pty::native_pty_system()
            .openpty(portable_pty::PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("terminal");
        let termios = format!("{:?}", pair.master.get_termios());
        let stdout = world.project.config_home().join("prompt-stdout");
        let mut command = portable_pty::CommandBuilder::new("sh");
        command.args(["-c", "exec \"$@\" > \"$PROMPT_STDOUT\"", "prompt-test"]);
        command.arg(assert_cmd::cargo::cargo_bin!("git-tpl"));
        command.args(args);
        command.cwd(&world.project.path);
        command.env_clear();
        // Keep the instrumented child's profile in llvm-cov's collection path;
        // clearing this would make real prompt interactions invisible to coverage.
        if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
            command.env("LLVM_PROFILE_FILE", profile);
        }
        command.env("PATH", std::env::var_os("PATH").expect("PATH"));
        command.env("HOME", world.project.config_home());
        command.env("XDG_CONFIG_HOME", world.project.config_home());
        command.env("GIT_CONFIG_NOSYSTEM", "1");
        command.env(
            "GIT_CONFIG_GLOBAL",
            world.project.config_home().join("absent.gitconfig"),
        );
        command.env("TERM", "xterm-256color");
        command.env("PROMPT_STDOUT", &stdout);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();
        let child = pair.slave.spawn_command(command).expect("git-tpl");
        drop(pair.slave);
        let (send, chunks) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buffer = [0; 4096];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 || send.send(buffer[..count].to_vec()).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            master: pair.master,
            writer,
            chunks,
            output: Vec::new(),
            consumed: 0,
            stdout,
            termios,
        }
    }

    fn expect(&mut self, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(position) = self.output[self.consumed..]
                .windows(text.len())
                .position(|w| w == text.as_bytes())
            {
                self.consumed += position + text.len();
                return;
            }
            let chunk = self
                .chunks
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|error| {
                    panic!(
                        "waiting for {text:?}: {error}\n{}",
                        String::from_utf8_lossy(&self.output)
                    )
                });
            self.output.extend(chunk);
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        if bytes == b"\x03" {
            // Console restores cooked mode between individual reads. Sending
            // Ctrl-C during a redraw would signal the PTY's process group,
            // bypassing the library's raw-key handling and testing a race.
            let deadline = Instant::now() + Duration::from_secs(5);
            while format!("{:?}", self.master.get_termios()) == self.termios {
                assert!(
                    Instant::now() < deadline,
                    "prompt never entered raw input mode"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
    }

    fn answer(&mut self, prompt: &str, bytes: &[u8]) {
        self.expect(prompt);
        self.send(bytes);
    }

    fn finish(mut self) -> (portable_pty::ExitStatus, String, String) {
        let deadline = Instant::now() + Duration::from_secs(20);
        let status = loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "child did not exit: {}",
                String::from_utf8_lossy(&self.output)
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        // A reader can still be draining after wait; channel closure is the
        // end-of-output boundary, not the process exit status.
        while let Ok(chunk) = self.chunks.recv_timeout(Duration::from_secs(2)) {
            self.output.extend(chunk);
        }
        assert_eq!(
            format!("{:?}", self.master.get_termios()),
            self.termios,
            "terminal attributes were not restored"
        );
        (
            status,
            String::from_utf8_lossy(&self.output).into_owned(),
            std::fs::read_to_string(&self.stdout).unwrap(),
        )
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Failed prompt assertions must not leave an interactive child behind.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn render(world: &World, color: &str) -> Terminal {
    Terminal::start(
        world,
        &[
            "--color",
            color,
            "--json",
            "render",
            &world.template.source(),
            "-o",
            "rendered",
        ],
    )
}

#[test]
fn duplicate_labels_keep_the_default_value_and_prompt_help_stays_off_stdout() {
    let world = World::with_template(
        "name = 'labels'\n[questions.pick]\ntype = 'choice'\nprompt = 'Pick label'\nhelp = 'Full question explanation'\nchoices = [{value='a',label='Same',help='First option'}, {value='b',label='Same',help='Second option'}]\ndefault = 'b'\n",
        &[("answer.jinja", "{{ pick }}")],
    );
    let mut terminal = render(&world, "never");
    terminal.expect("Full question explanation");
    terminal.answer("Second option", b"\r");
    let (status, stderr, stdout) = terminal.finish();
    assert!(status.success(), "{stderr}");
    assert_eq!(world.project.read("rendered/answer"), "b");
    serde_json::from_str::<serde_json::Value>(&stdout).expect("uncontaminated JSON");
    assert!(
        !regex::Regex::new(r"\x1b\[[0-9;]*m")
            .unwrap()
            .is_match(&stderr),
        "{stderr:?}"
    );
}

#[test]
fn typeahead_filters_values_and_fuzzy_search_also_works_for_data_driven_choices() {
    let sgr = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    for dynamic in [false, true] {
        let source = if dynamic {
            "choices_from = 'data.options'\nfuzzy = true"
        } else {
            "choices = ['cargo build', 'cargo test']"
        };
        let data = if dynamic {
            "[data.options]\nsource = 'options.json'\n"
        } else {
            ""
        };
        let world = World::with_template(
            &format!(
                "name = 'search'\n{data}[questions.pick]\ntype = 'choice'\nprompt = 'Search commands'\n{source}\n"
            ),
            &[("answer.jinja", "{{ pick }}")],
        );
        if dynamic {
            world
                .template
                .repo
                .write("options.json", "[\"cargo build\",\"cargo test\"]")
                .commit_all("choice data");
        }
        let mut terminal = render(&world, "always");
        terminal.answer("cargo test", if dynamic { b"ct\r" } else { b"test\r" });
        let (status, stderr, stdout) = terminal.finish();
        assert!(status.success(), "{stderr}");
        assert_eq!(world.project.read("rendered/answer"), "cargo test");
        assert!(sgr.is_match(&stderr), "{stderr:?}");
        serde_json::from_str::<serde_json::Value>(&stdout).unwrap();
    }
}

#[test]
fn filtering_keeps_hidden_checked_items_in_original_order() {
    let world = World::with_template(
        "name = 'multi'\n[questions.pick]\ntype = 'multi_choice'\nchoices = ['alpha','beta','gamma']\ndefault = ['gamma','alpha']\n",
        &[("answer.jinja", "{{ pick | join(',') }}")],
    );
    let mut terminal = render(&world, "never");
    terminal.answer("gamma", b"beta \r");
    let (status, stderr, _) = terminal.finish();
    assert!(status.success(), "{stderr}");
    assert_eq!(world.project.read("rendered/answer"), "alpha,beta,gamma");
}

#[test]
fn resizing_a_narrow_terminal_keeps_typed_unicode_choices_and_complete_prompt_help() {
    let help = "The complete question explanation remains available above the height-limited selection widget.";
    let world = World::with_template(
        &format!(
            "name = 'resize'\n[questions.pick]\ntype = 'choice'\nhelp = '{help}'\nchoices = [{{value='a',label='café 🌍'}},{{value='b',label='日本語 🌱'}}]\ndefault = 'b'\n"
        ),
        &[("answer.jinja", "{{ pick }}")],
    );
    let mut terminal = render(&world, "never");
    terminal.expect(help);
    terminal.expect("日本語");
    terminal
        .master
        .resize(portable_pty::PtySize {
            rows: 6,
            cols: 24,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    // Resize is picked up on the next redraw. Move away and back to exercise
    // layout without changing the default's original typed identity.
    terminal.send(b"\x1b[A\x1b[B\r");
    let (status, stderr, _) = terminal.finish();
    assert!(status.success(), "{stderr}");
    assert_eq!(world.project.read("rendered/answer"), "b");
}

#[test]
fn escape_clears_a_query_before_cancelling_and_ctrl_c_cancels_immediately() {
    for cancel in [b"\x1b".as_slice(), b"\x03".as_slice()] {
        let world = World::with_template(
            "name = 'cancel'\n[questions.pick]\ntype = 'choice'\nchoices = ['alpha','beta']\n",
            &[("answer.jinja", "{{ pick }}")],
        );
        let mut terminal = render(&world, "never");
        // One key produces one no-match redraw. Queuing several lets the first
        // redraw satisfy expect while later keys still switch terminal modes,
        // so Ctrl-C can signal the process group instead of reaching the widget.
        terminal.answer("beta", b"z");
        terminal.expect("No matching options");
        if cancel == b"\x1b" {
            terminal.send(b"\x1b");
            terminal.expect("beta");
        }
        terminal.send(cancel);
        let (status, stderr, stdout) = terminal.finish();
        assert!(!status.success(), "{stderr}");
        assert!(status.signal().is_none(), "{status}");
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(json["error"]["code"], "tpl::eval::cancelled");
        assert!(
            stderr.contains("\x1b[?25h"),
            "cursor not restored: {stderr:?}"
        );
    }
}

#[test]
fn input_normalization_validation_and_list_retries_preserve_defaults() {
    let world = World::with_template(
        "name = 'input'\n[questions.name]\ntype = 'string'\nprompt = 'Name input'\ndefault = 'valid'\npattern = '^[a-z]+$'\nmessage = 'lowercase only'\n[questions.port]\ntype = 'integer'\nprompt = 'Port input'\ndefault = 42\n[questions.modules]\ntype = 'list'\nprompt = 'Module input'\ndefault = ['one','two','three']\nitem_pattern = '^[a-z]+$'\nitem_message = 'module lowercase'\n",
        &[(
            "answer.jinja",
            "{{ name }}:{{ port }}:{{ modules | join(',') }}",
        )],
    );
    let mut terminal = render(&world, "never");
    terminal.answer("Name input", b"INVALID\r");
    terminal.expect("is not a valid answer");
    terminal.answer("Name input", b"   \r");
    terminal.answer("Port input", b"nope\r");
    terminal.answer("Port input", b"\r");
    terminal.answer("Module input #1", b"INVALID\r");
    terminal.expect("module lowercase");
    terminal.answer("Module input #1", b"\r");
    terminal.answer("Module input #2", b"-\r");
    terminal.answer("Module input #3", b"\r");
    terminal.answer("Module input #4", b"\r");
    let (status, stderr, _) = terminal.finish();
    assert!(status.success(), "{stderr}");
    assert_eq!(world.project.read("rendered/answer"), "valid:42:one,three");
}

#[test]
fn hunk_defaults_empty_confirmation_and_cancellation_keep_mailbox_stdout_clean() {
    for input in [b"\r".as_slice(), b" \r".as_slice(), b"\x1b".as_slice()] {
        let world = World::with_template("name = 'hunks'\n", &[("notes", "original\n")]);
        world.init(&[]).success();
        world.project.write("notes", "changed\n");
        let mut terminal = Terminal::start(&world, &["--color", "never", "backport", "-p"]);
        terminal.answer("Send which hunks", input);
        let (status, stderr, stdout) = terminal.finish();
        if input == b"\r" {
            assert!(status.success(), "{stderr}");
            assert!(stdout.starts_with("From "), "{stdout}");
            let patch = world.dir.path().join("selected.mbox");
            std::fs::write(&patch, stdout).unwrap();
            world.template.repo.git(&["am", patch.to_str().unwrap()]);
            assert_eq!(world.template.repo.read("template/notes"), "changed\n");
        } else {
            assert!(stdout.is_empty(), "{stdout}");
            assert_eq!(status.success(), input != b"\x1b", "{stderr}");
        }
    }
}

#[test]
fn nonterminal_input_fails_cleanly_instead_of_reading_piped_answers() {
    let world = World::with_template(
        "name = 'tty'\n[questions.name]\ntype = 'string'\n",
        &[("answer.jinja", "{{ name }}")],
    );
    assert_eq!(
        tpl(
            &world.project,
            &["--json", "render", &world.template.source(), "-o", "out"]
        )
        .failure()
        .error_code(),
        "tpl::eval::cancelled"
    );
}

#[test]
fn personal_seeds_and_boolean_defaults_are_accepted_only_after_prompting() {
    let world = World::with_template(
        "name = 'seeds'\n[questions.name]\ntype = 'string'\nprompt = 'Seeded name'\ndefault = 'template'\n[questions.enabled]\ntype = 'boolean'\nprompt = 'Enable feature'\ndefault = true\n",
        &[("answer.jinja", "{{ name }}:{{ enabled }}")],
    );
    world.project.user_config("[defaults]\nname = 'personal'\n");
    let mut terminal = render(&world, "never");
    terminal.answer("Seeded name [personal]", b"\r");
    terminal.answer("Enable feature", b"\r");
    let (status, stderr, _) = terminal.finish();
    assert!(status.success(), "{stderr}");
    assert_eq!(world.project.read("rendered/answer"), "personal:True");
    // The same preset must reach dialoguer's Input and choisir's Confirm,
    // including the answered state rather than only the pending question.
    assert!(stderr.contains("Seeded name [personal] ›"), "{stderr}");
    assert!(stderr.contains("Seeded name · personal"), "{stderr}");
    assert!(stderr.contains("Enable feature ›"), "{stderr}");
    assert!(stderr.contains("Enable feature · Yes"), "{stderr}");
}

#[test]
fn cancelling_network_consent_never_fetches_or_silently_skips_the_source() {
    let server = common::TestServer::start(vec![("/data.json", 200, "42".into())]);
    let world = World::with_template(
        &format!(
            "name = 'consent'\n[data.remote]\nsource = '{}'\n",
            server.url("/data.json")
        ),
        &[("answer.jinja", "{{ data.remote }}")],
    );
    let mut terminal = render(&world, "never");
    terminal.expect("This template wants to reach the network");
    terminal.expect("Each response is limited");
    terminal.answer("Abort", b"\x1b");
    let (status, stderr, stdout) = terminal.finish();
    assert!(!status.success(), "{stderr}");
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["error"]["code"], "tpl::data::cancelled");
    assert_eq!(server.hits(), 0);
}

#[test]
fn reversal_confirmation_preserves_enter_acceptance_and_cancellation_decline() {
    for input in [b"\r".as_slice(), b"\x1b".as_slice(), b"\x03".as_slice()] {
        let world = reversal_world();
        let mut terminal = Terminal::start(&world, &["--color", "never", "backport"]);
        terminal.answer("Send this line upstream?", input);
        let (status, stderr, stdout) = terminal.finish();
        if input == b"\r" {
            assert!(status.success(), "{stderr}");
            assert!(stdout.contains("+{{ name }} is improved"), "{stdout}");
        } else {
            assert!(stdout.is_empty(), "{stdout}");
            assert!(status.signal().is_none(), "{status}");
            assert!(stderr.contains("substituted_region"), "{stderr}");
        }
    }
}

fn reversal_world() -> World {
    let world = World::with_template(
        "name = 'reverse'\n[questions.name]\ntype = 'string'\ndefault = 'acme'\n",
        &[("notes.jinja", "{{ name }} is original\n")],
    );
    world.init(&[]).success();
    world.project.write("notes", "acme is improved\n");
    world
}

/// Characterize a trial blocker rather than masking it with a process-wide
/// signal handler: text input cannot return its intended domain outcome.
/// Replace these expectations with typed cancellation after fixing the
/// dialoguer integration; this is not a desired public contract.
#[test]
fn dialoguer_text_ctrl_c_currently_terminates_instead_of_returning_to_the_adapter() {
    let text = World::with_template(
        "name = 'interrupt'\n[questions.name]\ntype = 'string'\nprompt = 'Interrupt text'\n",
        &[("answer.jinja", "{{ name }}")],
    );
    let mut terminal = render(&text, "never");
    terminal.answer("Interrupt text", b"\x03");
    let (status, stderr, stdout) = terminal.finish();
    assert!(status.signal().is_some(), "{status}: {stderr}");
    assert!(
        stdout.is_empty(),
        "unexpected structured cancellation: {stdout}"
    );
}

#[test]
fn boolean_confirmations_support_arrow_navigation_and_clean_cancellation() {
    for input in [
        b"\x1b[C\r".as_slice(),
        b"\x1b".as_slice(),
        b"\x03".as_slice(),
    ] {
        let world = World::with_template(
            "name = 'confirm'\n[questions.enabled]\ntype = 'boolean'\nprompt = 'Enable feature'\ndefault = true\n",
            &[("answer.jinja", "{{ enabled }}")],
        );
        let mut terminal = render(&world, "never");
        terminal.answer("Enable feature", input);
        let (status, stderr, stdout) = terminal.finish();
        assert!(status.signal().is_none(), "{status}: {stderr}");
        let json: serde_json::Value = serde_json::from_str(&stdout).expect("uncontaminated JSON");
        if input == b"\x1b[C\r" {
            assert!(status.success(), "{stderr}");
            assert_eq!(world.project.read("rendered/answer"), "False");
        } else {
            assert!(!status.success(), "{stderr}");
            assert_eq!(json["error"]["code"], "tpl::eval::cancelled");
        }
        assert!(
            stderr.contains("\x1b[?25h"),
            "cursor not restored: {stderr:?}"
        );
    }
}
