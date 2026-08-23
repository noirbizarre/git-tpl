# Roadmap

There is no roadmap file. There is an issue tracker.

What git-tpl does is what these pages describe: if a behaviour is documented, it is implemented and tested.
What it does not do yet is [in the tracker][issues], one issue per thing, each carrying the reasoning rather than
a line item.

This page used to be `PLAN.md`, a file in the repository listing everything not yet built.
It was accurate for as long as somebody remembered to edit it, which is the failure mode of every such file.
A plan that describes an older version of the project is worse than none, because it is believed.

## Finding your way around

Issues carry an `area:` label matching the layout in `AGENTS.md`:

| Label | What it covers |
|---|---|
| [`area:cli`][area-cli] | Commands, arguments and output |
| [`area:render`][area-render] | Template manifest, evaluation and rendering |
| [`area:git`][area-git] | The Git backend and refs |
| [`area:data`][area-data] | Data sources |
| [`area:docs`][area-docs] | This site |
| [`area:packaging`][area-packaging] | Distribution and release artefacts |
| [`area:testing`][area-testing] | Test suite and harness |

Two labels are worth knowing about:

- [`adr-needed`][adr-needed] — the design is not settled. These need an argument before they need code, and that
  argument belongs in an [ADR](../adr/README.md).
- [`good first issue`][good-first-issue] — small, self-contained, and the surrounding code is already understood.

## Decided, and not changing

Some things are absent on purpose.
Proposing them again costs everyone an afternoon, so they are written down.

The [declined features](contributing.md#things-that-will-be-declined) list covers the big ones — custom merge
logic, a second template engine, code execution from templates, runtime values in the render context, automatic
ref push and fetch, Copier compatibility.

Two more are deliberate trade-offs rather than non-goals, and both have the same shape:

**The template is cloned fresh on every run.** Correct and slow for a large remote template.
A cache would need invalidation, and a stale cache silently rendering an old template is a far worse failure than
a slow fetch — so this stays until it actually hurts.

**A remote data source is fetched on every run.** Once per run, never cached between runs, for the same reason.
`sha256` is the answer for anyone who needs the bytes to be the same ones.

Both are cost, knowingly paid, in a tool whose whole premise is reproducibility.
If you need them changed, the case to make is that the cost has become real — not that the fetch is redundant,
which is understood.

## Changing your mind, or ours

A hard-to-reverse decision arrives as an [ADR](../adr/README.md): the context, the decision, and the consequences
somebody will live with.
Superseding an existing ADR is the supported way to reverse one; a PR that quietly works around it is not.

[issues]: https://github.com/noirbizarre/git-tpl/issues
[area-cli]: https://github.com/noirbizarre/git-tpl/labels/area%3Acli
[area-render]: https://github.com/noirbizarre/git-tpl/labels/area%3Arender
[area-git]: https://github.com/noirbizarre/git-tpl/labels/area%3Agit
[area-data]: https://github.com/noirbizarre/git-tpl/labels/area%3Adata
[area-docs]: https://github.com/noirbizarre/git-tpl/labels/area%3Adocs
[area-packaging]: https://github.com/noirbizarre/git-tpl/labels/area%3Apackaging
[area-testing]: https://github.com/noirbizarre/git-tpl/labels/area%3Atesting
[adr-needed]: https://github.com/noirbizarre/git-tpl/labels/adr-needed
[good-first-issue]: https://github.com/noirbizarre/git-tpl/labels/good%20first%20issue
