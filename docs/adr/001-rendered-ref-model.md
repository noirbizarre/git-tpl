# ADR-001: The rendered state of a template is a Git ref

**Status:** accepted

## Context

A project generated from a template needs to receive template updates later.
That is the hard part — generating files once is trivial.

The established approach (Copier, Cruft, Yeoman) is for the tool to own the update: keep a copy of, or a
description of, the previously generated output; re-render; compute a difference; apply it to the user's files.
Every tool in this space then spends most of its complexity budget on that application step — three-way
reconstruction, patch replay, conflict markers, rename detection.

We have a system that does all of this, extremely well, and that the user already has installed and already
understands.

## Decision

The rendered output of a template is a commit on a dedicated ref, `refs/tpl/<template-id>`.
Updating the template adds a commit to that ref.
The user incorporates changes with a normal `git merge`.

```
main:  A ─── B ─── C ─── D ─── M
            /                 /
       G0 ─┴──────── G1 ─── G2
```

git-tpl's entire job is to produce the desired rendered Git state.
Once that state exists, Git takes over.

## Consequences

The previous rendering is a real commit, so a merge has a real base.
Your edits and the template's changes are just two sides of a three-way merge.

Everything in Git works, unchanged: `git log refs/tpl/<id>`, `git diff HEAD refs/tpl/<id>`, `git merge`, `--abort`,
`rerere`, `mergetool`, `.gitattributes` merge drivers, signed commits, `bisect`.

There is no sidecar state.
Nothing can disagree with anything, because there is only one record.

The tool is small.
Most of what a template tool normally does is not here.

The cost: rendering must be deterministic, because a rendering that varied would create a commit on every run
(ADR-006).
And the model requires the user to understand that `refs/tpl/<id>` exists — which the documentation leads with,
rather than hiding.
