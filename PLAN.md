# PLAN

The living record of what is built, what is not, and what comes next.

Updated with the code, in the same commit. A plan that describes an older
version of the project is worse than none, because it is believed.

---

## Where things stand

**The core model works end to end.** `init → update → diff → merge` on real
repositories, with real libgit2 merges and real conflicts. 311 tests pass.

```
main:  A ─── M ─── B ─── M'
            /           /
       G0 ─┴─── G1 ────┘        refs/tpl/<id>
```

### Implemented

| Area | State |
|---|---|
| Rendered refs, `refs/tpl/<id>` | ✅ append-only, never force-updated |
| `init` | ✅ orphan commit + unrelated-histories merge |
| `update` | ✅ ref only — HEAD, index and worktree untouched (asserted) |
| `status` | ✅ text and `--format json`, exit 2 when pending |
| `diff` | ✅ patch, `--stat`, `--name-only`, path limiting |
| `merge` | ✅ libgit2 merge, `--abort`, zero custom conflict logic |
| `fetch` / `push` | ✅ explicit refspecs, never forced, divergence refused |
| Questions | ✅ string, boolean, integer, choice, multi_choice |
| Conditional questions | ✅ skipped questions are *absent*, not null |
| Dynamic defaults | ✅ evaluated at prompt time |
| `choices_from` | ✅ structured reference into the context |
| Computed values | ✅ typed, dependency-ordered |
| Dependency graph | ✅ static, topologically sorted, cycles and typos rejected up front |
| Template data sources | ✅ read from the template's Git tree at the resolved revision |
| Project data sources | ✅ with traversal refused |
| TOML and JSON | ✅ types preserved |
| Provenance | ✅ commit trailers, round-tripped |
| Determinism | ✅ asserted by test; no runtime context exists |
| Local template development | ✅ including `--dirty` |
| `.config/git.tpl.toml` | ✅ written, committed, hand-editable |
| Git config (`tpl.*`) | ✅ Git's own precedence |
| Git backend isolation | ✅ enforced by a prek hook, not by hope |
| Documentation | ✅ full Zensical site, 11 ADRs |
| Tooling | ✅ mise, prek, git-cliff, gh-ship, CI |
| Releases | ✅ binaries for six targets, plus crates.io via Trusted Publishing |

### Not implemented

| Area | Why, and what exists instead |
|---|---|
| **Remote data sources** | Declaring one produces a clear error naming the source. The `DataSource` abstraction and the `Data-Source` trailer already account for them, so this is additive. Template data is pinned by the template revision and has no equivalent problem. |
| **Data pinning** (checksums, Git-hosted data) | Designed in `docs/data/reproducibility.md`. Needs remote data first. |
| **`gh-tpl`** | Dropped from the bootstrap. Returns as a second `[[bin]]`, or by promoting the package to a workspace member — mechanical, no code movement. See ADR-004. |
| **SSH integration tests** | The credential path is implemented (agent → default keys → helper) and auth failures are translated into actionable diagnostics. Not exercised against a real host, because CI must not depend on anyone's private credentials. |
| **`git tpl show`, `git tpl detach`** | No clear semantics yet. Not implemented until there are. |

---

## Next

### 1. Remote data sources

The one declared feature that errors rather than working.

- An HTTP client behind the existing `DataSource` abstraction — nothing above
  `src/data/` should change.
- Size limits and a timeout. Remote data is untrusted input.
- One fetch per source per run; the cache already keys on the resolved source.
- Then `sha256 = "..."` pinning, and the `Data-Source` trailer records it.

Everything is already shaped for this. `SourceKind::Remote` exists, the
provenance format describes it, and `docs/data/remote.md` specifies the
behaviour including the failure mode.

### 2. SSH verification

Not a CI job. A documented procedure a developer can run against a private
repository of their own:

- `SSH_AUTH_SOCK` and a running agent
- a passphrase-protected key loaded into the agent
- `git@github.com:` URLs
- multiple identities, custom `~/.ssh/config`, non-standard ports

The credential callback tries the agent first precisely so a passphrase never
has to be typed. That is the part worth confirming by hand.

### 3. `gh-tpl`

`gh extension install` requires a repository named `gh-tpl` shipping a binary
named `gh-tpl`, and this repository is `noirbizarre/git-tpl`. Options, in order of
preference:

1. A thin `noirbizarre/gh-tpl` repository whose release assets are built from this
   one.
2. Build `gh-tpl` here and attach it to releases; document manual installation.

The CLI layer is already thin enough that the second binary is a `main.rs` and
a different `bin_name`.

### 4. Smaller things

- `git tpl show <path>` — the template's version of one file. Wanted whenever a
  merge conflicts.
- `--stat` should count lines, not just files. Needs a line-level diff summary
  from libgit2.
- Snapshot tests over CLI output. The harness and `insta` are already wired.
- A wordmark logo. `docs/images/icon.svg` is the mark; `mise run social`
  renders the social preview from it, and the wordmark is currently `<text>`.

---

## Deliberately not planned

Not oversights. Each is a decision, and most have an ADR.

- **Custom merge or reconciliation** — ADR-002. Git does this.
- **A second template engine** — ADR-003.
- **Code execution from templates** — no hooks, no scripts, no interpreter.
  Templates are untrusted input.
- **Runtime context** (`now()`, `git.user`, environment) — ADR-006. A value
  that varies by machine belongs in the answers.
- **Automatic push/fetch of template refs** — explicit by design.
- **Copier or Cruft compatibility** — a useful reference, not a goal.
- **A gitoxide backend** — libgit2 is the backend. The abstraction keeps the
  option open; nobody is taking it.
- **Template registries, package managers, a web UI** — a template is a Git
  repository. That is the whole distribution mechanism.

---

## Known rough edges

**`git tpl diff` lists every file the project has that the template does not.**
Including `.config/git.tpl.toml` and anything the user created. This is the
correct semantic — it is a tree diff — and it is documented, but the first
`diff` on a real project is noisier than it looks like it should be. A
`--template-only` mode limiting the diff to paths the template owns would help.

**The template is cloned fresh on every run.** Correct and slow for a large
remote template. A cache would need invalidation, and a stale cache silently
rendering an old template is a far worse failure than a slow fetch — so this
stays until it actually hurts.

**`--stat` reports file counts, not line counts.** Honest but less useful than
`git diff --stat`.

**Windows exec bits.** Rendering records `false` for every file on Windows,
matching what Git records there. A tree built on Windows and one built on Linux
from the same template will therefore differ in mode. Not yet exercised by a
test that would catch a regression.

---

## Invariants

Enforced, not merely intended. Each fails a hook or a test.

1. **`update` does not modify `HEAD`, the index or the worktree.**
   `tests/update.rs::update_does_not_touch_head_the_index_or_the_worktree`
   fingerprints all three. Structural, too: the renderer writes to a Git tree
   builder and never to the filesystem.

2. **Rendering is deterministic.** Identical inputs, identical tree. Asserted in
   `src/render.rs`, and observable from outside — an unchanged template makes no
   commit.

3. **Template refs are append-only.** Never amended, never force-pushed. The
   parent is always the previous tip.

4. **`git2` appears only in `src/git/libgit2.rs`.** The `git-backend-isolation`
   prek hook.

5. **No code execution from templates.** No subprocess, no shell, no eval, no
   HTTP outside `src/data/`.
