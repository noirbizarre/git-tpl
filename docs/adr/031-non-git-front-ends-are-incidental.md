# ADR-031: A Git front-end works by riding ADR-001 and ADR-011, not by being supported

**Status:** accepted

## Context

[#74](https://github.com/noirbizarre/git-tpl/issues/74) asked what git-tpl's stance is toward Jujutsu. A colocated
`jj git init` workspace works today, with no code written for it: `.git` is real, and everything git-tpl does —
writing `refs/tpl/<id>`, asking for a `git merge` — is plain Git ([ADR-001](001-rendered-ref-model.md)) through the
one backend module that speaks it ([ADR-011](011-git-backend-isolation.md)). A non-colocated workspace fails,
because there is no `.git` to find.

The question generalises past jj: Sapling, and anything else that can present itself as "a Git repository plus its
own front-end," has the same two properties for the same structural reason — free compatibility in the colocated
shape, none in the non-colocated one. It is worth deciding once rather than re-litigating per tool.

## Decision

git-tpl targets Git. A front-end is never named in `src/`, never detected beyond the narrow diagnostic purpose of
telling a user what to do next (see `GitError::JujutsuNotColocated`), and never gets a second `GitBackend`
implementation to accommodate it — ADR-011 already declined that door for reasons that have nothing to do with any
particular front-end. When a front-end leaves behind a real, discoverable `.git`, git-tpl works on it, because
ADR-001's whole premise is that git-tpl produces ordinary Git state and stops there. When it does not, git-tpl
reports plainly that it found no Git repository, with whatever pointer toward the front-end's own colocation mode
it can give cheaply — not a workaround, and not a probe for what the front-end actually is.

A front-end's own footguns — jj's `MERGE_HEAD` blindness turning `jj commit` mid-merge into data loss, most
notably — are documented (`docs/concepts/git-model.md`) as a thing to know, not a thing git-tpl works around.
There is no commit hook, no merge wrapper, and no state git-tpl could hold that would make a foreign tool's own
model compatible with Git's without duplicating the work that tool chose not to do.

## Consequences

A compatible front-end works, and keeps working, without git-tpl's engagement, because it rides an existing
invariant rather than a bespoke integration point built for it by name. The next front-end request — Sapling or
whatever comes after — has this ADR as its answer instead of a fresh judgement call.

The cost: a front-end's rough edges around the merge (jj's `MERGE_HEAD` gap, for one) are the user's to navigate
with that front-end's own commands, and a request to smooth them over from git-tpl's side is declined per this
ADR, not evaluated case by case.
