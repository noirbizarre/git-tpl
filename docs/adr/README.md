# Architecture decisions

Short records of decisions that are hard to reverse, where there was a real alternative, or where the reasoning
would otherwise be lost.

They are referenced by number from code comments, hooks, workflows and the rest of the documentation.
A decision that changes gets a new ADR that supersedes the old one; the old one stays, because the reasoning still
explains the code that existed.

| # | Decision |
|---|---|
| [001](001-rendered-ref-model.md) | The rendered state of a template is a Git ref |
| [002](002-no-custom-reconciliation.md) | No custom merge or reconciliation |
| [003](003-minijinja-only.md) | MiniJinja is the only template engine |
| [004](004-single-crate.md) | A single crate, not a workspace |
| [005](005-append-only-refs.md) | Template refs are append-only |
| [006](006-no-runtime-context.md) | No runtime values in the template context |
| [007](007-static-dependency-graph.md) | Question order is derived from a static dependency graph |
| [008](008-provenance-in-trailers.md) | Provenance lives in commit trailers, not in the tree |
| [009](009-init-merges-unrelated-histories.md) | `init` merges the template commit into the branch |
| [010](010-config-location.md) | Project configuration lives at `.config/git.tpl.toml` |
| [011](011-git-backend-isolation.md) | `git2` is confined to one module, enforced by a hook |
| [012](012-template-loader.md) | Partials are `.jinja` files outside the render root |
| [013](013-user-configuration.md) | User preferences live in `~/.config/git-tpl/config.toml` |
| [014](014-strict-undefined.md) | An undefined name in a rendered file may be an error |
| [015](015-machine-readable-output.md) | `--json` carries a stable diagnostic code on every failure |
| [016](016-template-tests-are-data.md) | Template tests are declarative cases in the template repository, with no code execution |
| [017](017-ignore-evaluation.md) | `.gitignore` is evaluated by us, not by libgit2 |
| [018](018-seed-context.md) | A prompt seed may be derived from the repository, through a closed context |
| [019](019-templates-address-never-act.md) | A template may address the user and declare Git remotes; it never runs anything |
| [020](020-backport-is-a-patch.md) | `backport` emits a patch, and proves it by re-rendering |
| [021](021-attachment-in-the-merge-commit.md) | The attachment rides in the merge commit, so `init` adds one commit |
| [022](022-backport-unsubstitutes.md) | Un-substitution is proved per line, and confirmed by a human |
| [023](023-hunk-selection-precedes-the-proof.md) | Hunk selection precedes the proof, and is taken on your own edits |
| [024](024-template-migrations.md) | A migration is a file, discovered by diffing the template's own history |
| [025](025-default-when-skipped.md) | A question may keep its default when skipped |
| [026](026-transparent-path-segments.md) | A rendered path piece may be `.` or fan out across `/`; only `..` and a backslash are rejected |
| [027](027-test-case-commands.md) | A test case may declare commands, run by the harness alone |
