# Determinism

The rendered output becomes a Git commit. That raises the bar: if rendering the
same inputs twice produced different bytes, every `git tpl update` would create a
commit, and every one of those commits would be noise you have to merge.

So the rule is absolute:

!!! info "The determinism guarantee"

    The same template revision, the same answers and the same data produce a
    byte-identical tree. Always, on every platform.

## What that requires

Each of these is a way the guarantee could be lost, and how it is prevented:

| Hazard | How it is handled |
|---|---|
| Directory traversal order | The template tree is walked in Git tree order, never filesystem order. `readdir` order varies by filesystem; Git's is sorted and canonical. |
| Line endings | Bytes are never translated. A `\r\n` in the template stays `\r\n` on Linux and on Windows. |
| File permissions | Only the executable bit is carried, taken from the source blob's Git mode. Git records nothing else, so nothing else can vary. |
| Binary files | Detected by a NUL byte in the first 8 KiB and copied verbatim, never rendered. |
| Timestamps | Never injected. The commit's author and committer time are the only timestamps, and they are metadata, not content. |
| Environment variables | Not exposed to templates. At all. |
| Machine-specific values | Not exposed to templates. See below. |

## No runtime context

git-tpl has no `now()`, no `git.user.name`, no `platform.os` — no expression can
read any of them.

This is a deliberate omission, not a gap. Every one of those makes rendering
depend on *when* and *where* it ran, which means two people running
`git tpl update` on the same commit get different trees, and the template ref
grows a commit every time anyone looks at it.

The usual motivations have better answers:

**A copyright year.** Ask for it, or don't render one — `Copyright (c) Acme` is
correct in every year.

**The author's name and email.** Ask a question, with
[`default_from = "git:user.name"`](../templates/questions.md#git-seeded-defaults),
which offers the value from your Git configuration as the *prompt default* — you
press Enter and move on. The difference is that the answer is then recorded in
`.config/git.tpl.toml`, and the next person to render gets your project's author
rather than their own. When nobody is asked, the seed is not read at all and the
question's own `default` applies, so the rule above is not weakened: nothing
machine-specific ever reaches a tree without a human accepting it first.

That is the general shape of the answer: a value that varies by machine belongs
in the answers, where it is recorded and shared, not in the context, where it is
invisible and different every time.

If a compelling case for runtime values appears, it will arrive as an explicit,
per-template opt-in that marks the template non-deterministic and records that
fact in the commit trailers. Not as an ambient global.

## Remote data is the remaining hazard

A template that loads data over HTTP is only as reproducible as that URL. Today
git-tpl records which data sources contributed to a rendering in the commit
trailers, so you can at least tell. Pinning by checksum is
[planned](../data/reproducibility.md).

Local data — files in the template repository — has no such problem. It is read
from the template's Git tree at the resolved revision, so it is pinned by the
template revision itself, exactly like the template files.

## Security

Determinism and safety turn out to be the same constraint viewed from two sides:
anything that could make rendering non-deterministic is also something that could
make it dangerous.

Templates are **untrusted input**. Rendering one must be as safe as reading a
text file. So there is no:

- Python execution
- shell execution
- template hooks or post-generation scripts
- subprocess execution of any kind
- arbitrary HTTP from within a template
- environment access

Dynamic behaviour is MiniJinja expressions over a controlled context, plus the
data-source subsystem, which owns *all* fetching. A template declares what data
it wants; it cannot reach out and take it.

Remote data is untrusted input too, and is never executable — it is parsed as
TOML or JSON into plain values, and that is the only thing that ever happens
to it.
