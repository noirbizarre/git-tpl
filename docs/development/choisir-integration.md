# Choisir integration

git-tpl uses the published choisir 0.1.0 crate with its `fuzzy` feature.
Choisir supplies selections and confirmations, dialoguer supplies text input, and console supplies terminal primitives.
Every widget uses choisir's `MinimalTheme`, including dialoguer's input prompts.
Console's stderr color policy keeps that shared presentation consistent with `--color`.

## Try it

```sh
mise run build
mise exec -- cargo test --test prompts
```

Create a template with inline choices or a `choices_from` data source, then run
`mise run cli -- render /path/to/template -o /tmp/choisir-output`.
Choice prompts use substring typeahead by default.
Set `fuzzy = true` on the question to try noncontiguous matching; it works for both choice kinds and both sources.
See [question search](../templates/questions.md#searching-choices-interactively).

For a terminal-driven reproduction matrix, run `mise exec -- cargo test --test prompts -- --nocapture`.
The tests use real Git repositories and a PTY, redirect stdout independently,
wait for prompts before sending keys, and verify terminal attributes after exit.
They cover default and personal-seed acceptance, duplicate labels, validation/list retries,
typeahead, fuzzy data choices, hidden checked values, color policy, consent cancellation,
backport hunk defaults/empty selection/cancellation, reversal approval, and Ctrl-C behavior.
Accepted hunk output is applied to the real template repository with `git am`.
These terminal tests are Unix-only; interactive behavior on Windows still needs a terminal run.

## Remaining integration gaps

### Prompt-level help: missing API, workaround available

`Select` and `MultiSelect` expose `item_with_description`, but no prompt-level description.
git-tpl needs that for question `help`, network source URLs, and hunk-selection instructions.
git-tpl prints complete help above the widget rather than dropping it or inserting newlines into the prompt.

Suggested API: a description builder shared by both widgets, with sanitization,
multiline/wrapping semantics, and height budgeting that keeps the highlighted option visible.
This would remove the external-help workaround.

Reproduction: give a choice question `help = "Explanation for the whole question"` and per-option help too.
The per-option description belongs to the widget, while question help currently sits outside its redraw region.

### Complete descriptions in narrow terminals: enhancement

Choisir flattens and width-limits item descriptions.
Long explanations can become unreadable without a way to expand or wrap them.
Critical consent information stays fully printed outside the widget in this integration.
Consider a wrapped or expanded-help mode with explicit height accounting when adding the description API.

Reproduction: use a long choice help string in a narrow terminal and compare it with the complete question help above it.

### Instance-level styling: enhancement, binary workaround verified

A supplied dialoguer theme does not control every choisir-owned footer/error style.
git-tpl synchronizes console's stderr color setting once at startup with its resolved `--color` policy.
PTY tests verify `--color never` emits no SGR styles and `--color always` emits them without polluting stdout.

An instance-level policy would let consumers style widgets independently without changing process-global settings,
particularly when passing a terminal other than stderr.
This is an API-control enhancement rather than a current git-tpl blocker.

### Already supported and verified

- Owned typed values independent of duplicate labels.
- Per-item help, explicit single-selection default and multiple preselected positions.
- Typeahead and optional fuzzy matching, including data-derived lists.
- Checked values surviving filtering and extraction in original input order.
- Escape clearing a query before cancelling, and Ctrl-C returning cancellation immediately.
- A confirmed empty multi-selection distinct from cancellation.
- Explicit stderr terminal injection, clean stdout, cursor restoration and restored terminal attributes.
- Arrow-enabled confirmations with Enter/default acceptance, Y/N shortcuts, and clean Escape/Ctrl-C cancellation.
- A shared `MinimalTheme` for choisir selections/confirmations and dialoguer input.

Search stays disabled for three-action consent menus and backport hunk menus.

## Dialoguer cancellation blocker

This affects only text input, which still uses dialoguer.
Dialoguer 0.12 uses console's ordinary key reads, which restore terminal attributes and raise SIGINT on Unix for Ctrl-C.
With the normal process signal disposition, git-tpl terminates before its adapter can map the result.

The PTY characterization test
`dialoguer_text_ctrl_c_currently_terminates_instead_of_returning_to_the_adapter`
verifies that text input exits by signal with no structured `tpl::eval::cancelled` JSON result.

Choisir confirmations resolve the earlier reversal-confirmation limitation:
Escape and Ctrl-C now decline a proposal cleanly, and Enter preserves demand's affirmative default.
Boolean questions likewise return typed cancellation on Escape or Ctrl-C and support arrow-key navigation.
Text input ignores Escape; use Ctrl-C to interrupt it.
Choisir's raw-key selection and confirmation paths handle Ctrl-C correctly and restore the cursor.

Before treating the migration as behavior-compatible, resolve cancellation in dialoguer or its integration.
A raw-key/optional-cancellation path upstream would avoid process-global signal handling or a custom input implementation.
The characterization assertions document the current blocker, not a permanent desired behavior;
replace them with typed-cancellation assertions when it is fixed.

## Intentional compatibility differences

Interactive widgets require stderr to be a terminal.
Demand's line-oriented fallback for redirected input/output is no longer used.
Stdout can still be redirected to JSON consumers or `git am`; prompts remain on stderr.
Automation should use explicit answers, answer files, or `--defaults`.
Selection search adds keys and makes the first Escape clear a nonempty query.
For multi-choice questions Space toggles rather than entering a query space, and descriptions are not searched.

## Published dependency

```toml
choisir = { version = "0.1.0", features = ["fuzzy"] }
```

`Cargo.lock` records the registry source and checksum; builds no longer require a local choisir checkout.
