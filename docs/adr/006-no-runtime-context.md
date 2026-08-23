# ADR-006: No runtime values in the template context

**Status:** accepted

## Context

Templates commonly want the current year for a copyright header, the author's name from Git configuration, or the
platform.
Copier and Cookiecutter provide these.

But the rendered tree becomes a Git commit (ADR-001), and `update` only commits when the tree changed (ADR-005).
A context value that varies by machine or by moment breaks that: everyone who runs `update` produces a different
tree, the ref grows a commit every time anyone looks at it, and each one is a merge the user has to perform for no
benefit.

## Decision

There is no runtime context.
No `now()`, no `git.user.name`, no `platform.os`, no environment access.

The context contains answers, computed values, loaded data and template metadata.
Nothing else.

## Consequences

Rendering is deterministic by construction rather than by convention.
There is no way for a template author to accidentally make their template non-reproducible, because the tools to
do so are absent.

`update` on an unchanged template is genuinely a no-op, so a clean `git tpl status` means something.

The use cases have better answers.
A copyright year should be asked for, or omitted — `Copyright (c) Acme` is correct every year.
An author name should be a question whose *default* the CLI fills from Git configuration: the user presses Enter,
and the value is then recorded in `.config/git.tpl.toml`, shared with the project, and identical for everyone who
renders it.
Which is what you wanted.
That is now implemented as
[`default_from = "git:user.name"`](../templates/questions.md#machine-seeded-defaults), and it upholds this decision
rather than relaxing it: the key is read only when a human is going to be asked, it seeds the prompt and never the
context, and a non-interactive render never reads it at all.

See also [ADR-018](018-seed-context.md), which elaborates that escape hatch into three named namespaces — the Git
configuration, the project directory name and the remote URL — and records why the set is closed.
It widens what a seed may be *derived from*; the two guards above are what it does not touch.

That is the general shape: a value that varies by machine belongs in the answers, where it is recorded and
reviewed, not in the context, where it is invisible.

If a compelling case appears, it arrives as a per-template opt-in that marks the template non-deterministic and
records that in the commit trailers — never as an ambient global.
The template metadata deliberately excludes the template's own revision for the same reason: a file containing its
own template's SHA would change on every template commit.
