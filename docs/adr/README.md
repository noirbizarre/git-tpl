# Architecture decisions

Short records of decisions that are hard to reverse, where there was a real
alternative, or where the reasoning would otherwise be lost.

They are referenced by number from code comments, hooks, workflows and the rest
of the documentation. A decision that changes gets a new ADR that supersedes the
old one; the old one stays, because the reasoning still explains the code that
existed.

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
