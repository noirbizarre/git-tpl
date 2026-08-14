# PLAN

The living record of what is built, what is not, and what comes next.

Updated with the code, in the same commit. A plan that describes an older
version of the project is worse than none, because it is believed.

---

## Where things stand

**The core model works end to end.** `init → update → diff → merge` on real
repositories, with real libgit2 merges and real conflicts. 474 tests pass.

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
| Adopting an existing project | ✅ `init` — the merge reconciles the two sides; no separate command |
| `update` | ✅ ref only — HEAD, index and worktree untouched (asserted) |
| `status` | ✅ text and `--format json`, exit 2 when pending |
| `diff` | ✅ patch, `--stat`, `--name-only`, path limiting |
| `merge` | ✅ libgit2 merge, `--abort`, zero custom conflict logic |
| `fetch` / `push` | ✅ explicit refspecs, never forced, divergence refused |
| Questions | ✅ string, boolean, integer, choice, multi_choice |
| Question validation | ✅ `pattern` + `message` on `string` questions; checked wherever an answer arrives, compiled at load time |
| Answers from a file | ✅ `--answers-from`, TOML/JSON/YAML, repeatable; unknown keys ignored and reported |
| Conditional questions | ✅ skipped questions are *absent*, not null |
| Dynamic defaults | ✅ evaluated at prompt time |
| Git-seeded defaults | ✅ `default_from = "git:<key>"` — seeds the prompt only, never a non-interactive render |
| `choices_from` | ✅ structured reference into the context |
| Computed values | ✅ typed, dependency-ordered |
| Template filters | ✅ MiniJinja's built-ins plus `slugify`; the set is closed, no plugin point |
| Shared macros | ✅ `{% import %}` and `{% include %}` against the template tree; a partial is a `.jinja` file outside the render root (ADR-012) |
| Dependency graph | ✅ static, topologically sorted, cycles and typos rejected up front |
| Template data sources | ✅ read from the template's Git tree at the resolved revision |
| Project data sources | ✅ with traversal refused |
| Remote data sources | ✅ size-limited, timed out, confirmed before the fetch |
| Data pinning | ✅ `sha256`, and the digest recorded in the trailer either way |
| TOML, JSON and YAML | ✅ types preserved; YAML 1.2, hostile documents refused |
| Provenance | ✅ commit trailers, round-tripped |
| Determinism | ✅ asserted by test; no runtime context exists |
| Local template development | ✅ including `--dirty` |
| `.config/git.tpl.toml` | ✅ written, committed, hand-editable |
| Git config (`tpl.*`) | ✅ Git's own precedence |
| Git backend isolation | ✅ enforced by a prek hook, not by hope |
| HTTP isolation | ✅ same, confining the client to `src/data/` |
| Documentation | ✅ full Zensical site, 12 ADRs |
| Tooling | ✅ mise, prek, git-cliff, gh-ship, CI |
| Releases | ✅ binaries for six targets, plus crates.io via Trusted Publishing |

### Not implemented

| Area | Why, and what exists instead |
|---|---|
| **Git-hosted data** (`source` + `ref` + `path`) | Designed in `docs/data/reproducibility.md`. `sha256` pinning covers the reproducibility case; this is about convenience for data that already lives in a repository. |
| **`gh-tpl`** | Dropped from the bootstrap. Returns as a second `[[bin]]`, or by promoting the package to a workspace member — mechanical, no code movement. See ADR-004. |
| **SSH integration tests** | The credential path is implemented (agent → default keys → helper) and auth failures are translated into actionable diagnostics. Not exercised against a real host, because CI must not depend on anyone's private credentials. |
| **`git tpl show`, `git tpl detach`** | No clear semantics yet. Not implemented until there are. |
| **Testing a template** | A template author renders into a scratch directory and looks. There is no `render` command and no test runner, so most templates have no tests. The fixtures a runner would read are now expressible — `--answers-from` ships. See *Next*. |
| **Template inheritance** | A template is one repository, standing alone. Fifteen templates in an organisation means fifteen copies of the same CI workflow. See *Next*. |
| **User configuration** | There is no `~/.config/git-tpl.toml`. Prompt defaults come from the template alone, template sources are written out in full every time, and remote-data trust is per-invocation with no persistent list. See *Next*. |
| **Distribution beyond crates.io** | `cargo install`, `mise use -g cargo:git-tpl`, `mise use -g github:noirbizarre/git-tpl` and raw release binaries. No AUR package, no Homebrew formula, no mise registry entry. See *Next*. |

---

## Next

### 1. A persistent trust list

Remote data sources ship, gated by a confirmation shown before any fetch, plus
`--trust` for one invocation. What is missing is the ability to say "I always
trust this organisation" once, instead of confirming the same template's network
access on every run.

That is a user-side fact — it belongs on the machine of the only party who can
consent to it, never in `.config/git.tpl.toml`, because a project cannot consent
on its reader's behalf. So it arrives with the user configuration file, as
`[trust].templates`, and is described in item 5 rather than duplicated here.

The gate it plugs into already exists: `TrustGate` in `src/data/`, selected in
`commands::trust`. A pattern match becomes one more arm alongside `--trust`.

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

### 4. Distribution: AUR and Homebrew

Cheap, because a release already produces six binaries and a `SHA256SUMS`. Both
packages are consumers of an artefact that exists; neither needs a build change.

**One upstream change makes all of them easier, and it should be decided
first.** The assets are bare binaries named `git-tpl_<tag>_<platform>`, so every
packaging format has to rename the file it downloads. Publishing `.tar.gz`
archives containing a plain `git-tpl` instead would make AUR, Homebrew, `ubi`,
`cargo-binstall` and a mise registry entry all trivial at once — and it is a
breaking change to the asset names, so it belongs to a version bump rather than
to a packaging PR.

**AUR — two packages, and the reason for two.** `git-tpl-bin` repackages the
`linux-amd64` and `linux-arm64` assets with `sha256sums` lifted from the
release; `git-tpl` builds with `cargo`, for people who will not run someone
else's binary. `publish-release.yaml` gains a job that pushes to
`ssh://aur@aur.archlinux.org/git-tpl-bin.git` **after** the assets are uploaded,
so a failed package never leaves a release advertising files that are not there.
The AUR SSH key is a repository secret and the account is a standing
maintenance obligation, not a one-off.

**Homebrew — a tap first.** `noirbizarre/homebrew-tap`, with a formula whose
`url` and `sha256` come from the release and a
`bin.install "git-tpl_#{version}_darwin-arm64" => "git-tpl"` until the archives
above exist. homebrew-core has an acceptance bar — notability, stable numbered
releases, no HEAD-only — that this project does not clear yet, and submitting
early wastes a reviewer's afternoon.

**A mise registry entry in the same pass.** `mise use -g cargo:git-tpl` and
`mise use -g github:noirbizarre/git-tpl` both work today and are documented. An
entry in `jdx/mise`'s `registry.toml` is what makes the short
`mise use -g git-tpl` work, and it consumes the same artefact as the other two.

All of this is downstream of a version somebody else depends on. Packaging
0.1.x means chasing it.

### 5. A user configuration file

Three unrelated frustrations with one home: retyping your name into every
project, typing `https://github.com/` twenty times a day, and confirming the
same template's network access on every run — the last of which is now a real
prompt rather than a hypothetical one, because remote data sources ship.

```
$XDG_CONFIG_HOME/git-tpl.toml     (default: ~/.config/git-tpl.toml)
```

**Three files, three owners.** This one looks like the project file and is not
it, so the split has to be stated before anything else:

```
.config/git.tpl.toml       →  the project. Versioned. Everyone gets it.
~/.config/git-tpl.toml     →  you. Never committed, never read by anyone else.
.git/config, ~/.gitconfig  →  tpl.* preferences (unchanged)
```

The naming difference is deliberate rather than an oversight: the project file
is Git-shaped and lives in `.config/` (ADR-010), the user file is named after
the binary and follows XDG. Two names means a stray copy of one is never
mistaken for the other. This is a new on-disk format, which is to say a
contract, so it needs **ADR-013** before it needs code.

```toml
[defaults]
author = "Axel Haustant"
email = "axel@example.com"
license = "MIT"

[shortcuts]
gh = "https://github.com/"
ghs = "ssh://git@github.com/"
mine = "https://github.com/noirbizarre/"

[trust]
templates = [
  "github.com/noirbizarre/*",
  "github.com/myorg/**",
]
```

Three sections, and deliberately nothing else. Anything that describes the
project belongs in `.config/git.tpl.toml`; anything Git already models belongs
in `tpl.*`.

#### `[defaults]` seeds prompts, never the context

This is the whole design, and the only thing that keeps invariant 2 true. It is
the implemented `default_from = "git:user.name"` rule applied to a second
source.

- A key matching a question name becomes that question's **prompt default**.
- The value the user accepts is recorded in `.config/git.tpl.toml` like any
  other answer, so the project remains reproducible for someone who has never
  seen your file.
- If the question is **not asked** — `--defaults`, `tpl.interactive false`, CI —
  the file is **ignored** and the template's own `default` applies. Otherwise
  the same template renders two different trees on two machines and the model
  breaks. This needs the test that says so, named for the claim:
  `user_defaults_do_not_apply_when_questions_are_not_asked`.
- Keys matching no question are ignored and reported, exactly as for
  `--answers-from`.
- Precedence, stated once and tested, extending the chain `--answers-from` established:

  ```
  --answer  >  --answers-from  >  answers in .config/git.tpl.toml
            >  [defaults]      >  default_from  >  the question's default
  ```

- No per-template namespacing in v1. If it proves necessary,
  `[defaults."github.com/org/*"]` is additive.

#### `[shortcuts]` expand before anything else sees the URL

Prefix substitution on a leading `<name>:`, performed in `ops` before the source
reaches `GitBackend` or `refs.rs`. One rule makes it safe:

> **The expanded URL is what gets written to `.config/git.tpl.toml`, and what
> derives the template id.** A shortcut never leaves your machine.

Without it, a project created by someone with `mine:` is unusable by everyone
else, and `refs/tpl/<id>` differs per contributor for the same template — an
invariant-3 problem wearing a convenience feature's clothes. Also:

- Shortcuts are matched only against the CLI argument, never against a value
  read out of a repository.
- `gh:` and `ghs:` are separate names rather than a scheme guessed from
  context. The reason for the ssh form is private repositories, and inferring
  which one you meant from whether a clone failed is exactly the retry logic
  that produces incomprehensible auth errors.
- An unknown `foo:` is left alone — it may be a real scheme. Only names present
  in the file expand.
- A name may not contain `/` and may not be a known scheme (`https`, `ssh`,
  `git`, `file`). Rejected when the file is read, not when it is used.

#### `[trust]` is a user-side fact and is never recorded in the project

Patterns match the **expanded** URL, normalised first: scheme, userinfo, port
and a trailing `.git` dropped, case folded. One entry therefore covers
`https://github.com/org/t`, `git@github.com:org/t.git` and `gh:org/t`. Globs
only — `*` within a path segment, `**` across segments. No regex and no
negation, because a trust list that needs debugging is a trust list that will
be got wrong.

#### What trust actually gates

**Rendering never requires trust.** Invariant 5 is untouched: no subprocess, no
eval, no code from a template, trusted or not. Trust gates only the things a
template asks *git-tpl* to do on its behalf, of which there are at most two:

| Capability | Status | Shown in full before it happens |
|---|---|---|
| **Remote data fetch** | ✅ implemented | every URL, the source name that declared it, the size limit |
| **Post-render tasks** | *Under review*, ADR-gated | every rendered command, in order |

When the template is **not** trusted:

1. Every requested capability is listed in full, before any of it happens.
2. You **pick per item** — accept, skip, or abort. Skipping a data source
   produces the failure the loader already reports by name, so no new failure
   mode is invented.
3. Nothing is remembered. The next run asks again.

When it **is** trusted — the URL matches a `[trust].templates` pattern, or
`--trust` was passed — everything is accepted without a prompt. `--trust` is
per-invocation and writes nothing anywhere.

Non-interactive and untrusted (`--defaults`, `tpl.interactive false`, CI): every
capability is **refused**, loudly, naming what was refused and saying that
`--trust` or a `[trust]` entry would allow it. Never silently accepted — a CI
runner is the worst possible place to grant a capability by omission.

Two things this is not, so it is not read as a weakening:

- Trust is not a security boundary against a template you have already decided
  to run. It is a confirmation step that makes the network and the shell
  visible before they are used.
- It does not create a place to record trust *in the project*. Trust lives on
  the machine of the only party who can consent to it.

### 6. Testing a template

A template author has no way to say *"given these answers, this is what comes
out"* except to render into a scratch directory and look. Every template above
a few files therefore has no tests, and the first person to find a broken
conditional is the person generating a project from it.

Everything needed already exists: rendering produces a tree without touching a
worktree, questions can be answered from a file, and diagnostics carry
stable codes.

**The primitive first: `git tpl render`.**

```sh
git tpl render <template> --answers-from answers.toml --output ./out
```

One template, one answer set, no project and no ref. Useful on its own — a CI
job that renders the template on every push catches a syntax error in a `.jinja`
file long before anyone runs `init`. This writes to a directory the user named,
which is **not** a hole in invariant 1: that invariant is about `update` never
touching `HEAD`, the index or the worktree, and this is a different command
whose entire purpose is stated in an explicit flag. Nothing about `update`
changes.

**Then `git tpl test`,** reading cases from the template repository:

```toml
# tests/minimal.toml
[answers]
project_name = "thing"
with_ci = false

[expect]
files = ["pyproject.toml", "src/thing/__init__.py"]
absent = [".github/workflows/ci.yml"]

[expect.contains]
"pyproject.toml" = ['name = "thing"']
```

Details that need deciding, because each is a place to get it wrong:

**Failure cases are asserted by diagnostic code, never by message text.**

```toml
[expect]
error = "tpl::questions::type"
```

A test suite that pins error prose makes every diagnostic improvement a
breaking change, which is how error messages stop improving. The codes are
already the stable surface — `tpl::<area>::<kind>` — so they are what a test
names.

**Snapshots, but reviewable.** `git tpl test --write` records the rendered tree
under `tests/__snapshots__/<case>/`, and a later run diffs against it. That is
`insta`'s model and it is the right one, but it must be implemented plainly
rather than by depending on `insta` — the snapshots belong to the user's
template repository, not to ours, and a template author should not need a Rust
toolchain to read them.

**One case per answer set, no matrix language.** Three files beat a
combinatorial `[[matrix]]` block that nobody can predict the expansion of. If a
template really needs twelve cases, twelve files say so honestly.

**Exit codes come from `exit.rs`,** unchanged, so `git tpl test` is usable in
CI without wrapping.

The fixtures *are* answers files, read by the same parsers — which is why
`--answers-from` had to come first.

### 7. Template inheritance

The largest template-side feature there is, and the one that decides whether an
organisation with fifteen templates maintains fifteen copies of its CI workflow.

```toml
# template.toml
[extends]
source = "https://github.com/org/base-template"
rev = "v3.1.0"
```

The child renders on top of the parent: it may add questions, data sources and
computed values, override any of them by name, add files, and replace files the
parent renders.

**The parent must be pinned, and this is not negotiable.** `rev` is required,
and it must resolve to an immutable revision — a tag or a commit, never a
branch. An unpinned parent means the same child revision renders two different
trees on two different days, and then "an unchanged template produces no commit"
is false, `update` produces mystery diffs, and invariant 2 has been traded for a
convenience. This is the same rule `docs/data/reproducibility.md` already states
for data, for the same reason, and it should read as one rule rather than two.

**Provenance records the whole chain.** A new `Template-Extends` trailer per
ancestor, in resolution order, each as `<url>@<sha>`. Without it, `git tpl
status` on a project cannot say what it was actually rendered from, and ADR-008
exists precisely so that question always has an answer.

**Merge semantics, one rule, applied everywhere:**

> **The unit of override is the name.** A child's `[questions.x]` replaces the
> parent's entirely. It does not merge field by field.

Field-level merging is the friendlier-looking choice and the wrong one: a child
that means to change a default would silently inherit a `when` clause it never
read, and debugging that means reading two repositories. Replacement costs the
author four lines of copying and costs the reader nothing. It applies uniformly
to `[questions]`, `[computed]` and `[data]`.

The rest of the manifest, stated so it is not guessed:

- **Question order.** Ancestors' questions come first, in ancestor order, then
  the child's new ones. An *overridden* question keeps the **parent's**
  position — inserting an override should not silently reshuffle the prompt
  sequence.
- **`name` and `description` are never inherited.** They describe this template.
- **`root` is per template.** Each layer renders its own tree from its own
  `root`; a child does not reach into the parent's directory layout.

**File layering, and the trap in it.** Each layer renders to its own set of
paths, then layers are overlaid in order with the child last. `render.rs`
already refuses two templates that render to the same path
(`two_templates_rendering_to_one_path_is_an_error`) — that check must stay
exactly as it is *within* a layer, and become "the child wins" *across* layers.
Conflating the two would either break the existing guarantee or make overriding
impossible, and it is the single most likely thing to get wrong here.

**Removing a parent's file needs to be explicit:**

```toml
[extends]
remove = ["template/.github/workflows/ci.yml.jinja"]
```

Parent-relative source paths, validated: removing a path the parent does not
have is an **error**. Otherwise a rename upstream silently resurrects a file the
child spent a release removing, and nobody notices until it ships.

**`{% extends %}` needs the loader, which now exists.** ADR-012 registered one,
backed by the template's own tree: a partial is a `.jinja` file outside the
render root, named by its path from the repository root. Inheritance needs the
same loader, backed by the *layered* trees, resolving child-first. One trap, and it is the
obvious one: a child's `base.html.jinja` writing `{% extends "base.html.jinja" %}`
must find the *parent's* file, not itself. Magic here produces infinite
recursion or, worse, silence — so the parent is named explicitly:

```jinja
{% extends "parent:base.html.jinja" %}
```

A loader that reads only from template trees executes nothing and reaches
nothing outside the pinned revisions, so it costs no invariant.

**Scope for v1, deliberately small:** a single chain, one parent per template.
No multiple inheritance and no diamonds — both raise a resolution-order question
that has no obvious right answer, and neither is needed by the case that
motivates this. Cycles are detected the way `graph.rs` detects them, up front
and by name, and the chain has a depth limit.

ADR-012 deliberately left the `<prefix>:` namespace free for exactly this, so
`parent:` costs no format change.

**It multiplies a known rough edge.** Every ancestor is cloned on every run, so
a three-deep chain is three clones. That makes the template cache — currently
"stays until it actually hurts" — start to hurt.

This changes the manifest, which is a contract, and changes what "the template"
means for provenance and for `refs/tpl/<id>`. It needs **ADR-014** before it
needs code.

### 8. Smaller things


- `git tpl show <path>` — the template's version of one file. Wanted whenever a
  merge conflicts.
- `--stat` should count lines, not just files. Needs a line-level diff summary
  from libgit2.
- Snapshot tests over CLI output. The harness and `insta` are already wired.
- Named tests for two behaviours that are currently correct but undefended: a
  computed value holding a sequence (`| select`) and one holding a table
  (`dict()`) both keep their type through `evaluate()`. Nothing would catch a
  regression that started stringifying them, and templates depend on both.
- A wordmark logo. `docs/images/icon.svg` is the mark; `mise run social`
  renders the social preview from it, and the wordmark is currently `<text>`.

---

## Deliberately not planned

Not oversights. Each is a decision, and most have an ADR.

- **Custom merge or reconciliation** — ADR-002. Git does this.
- **A second template engine** — ADR-003.
- **Code execution during rendering** — no hooks, no scripts, no interpreter, no
  filter plugin point. Templates are untrusted input, and this one is not up for
  discussion. (Tasks running *after* the merge are a separate question — see
  *Under review*.)
- **Runtime context** (`now()`, `git.user`, environment) — ADR-006. A value
  that varies by machine belongs in the answers.
- **Automatic push/fetch of template refs** — explicit by design.
- **A separate `adopt` command** — `init` on a populated repository already
  merges the orphan commit and lets Git reconcile the two sides. A second
  command was built, measured against the first, and found to differ only in
  its output. The conflicts are small because Git diffs content rather than
  ancestry — see the correction in ADR-009 and the tests in `tests/init.rs`.
  What was missing was documentation, not code.
- **Copier or Cruft compatibility** — a useful reference, not a goal. Reading
  `.copier-answers.yml` through the generic `--answers-from` is not
  compatibility; it is one of four things that flag buys.
- **A gitoxide backend** — libgit2 is the backend. The abstraction keeps the
  option open; nobody is taking it.
- **Template registries, package managers, a web UI** — a template is a Git
  repository. That is the whole distribution mechanism.

---

## Under review

**Opt-in post-render tasks.** Every real-world template surveyed so far runs the
same five commands after a first render: `git init`, an install, a hook install,
`git add`, `git remote add`. Today the answer is "the template renders a
`scripts/bootstrap.sh` and prints one line telling you to run it", which costs
the user one command and costs the project nothing.

Changing that contradicts a stated non-goal, so it arrives as an ADR superseding
it or not at all. What the ADR would have to establish:

- Tasks run in `ops`, after the merge — never inside the renderer. Invariants 1
  and 2 stay literally true, and their tests stay untouched. If either has to be
  relaxed, the feature is declined.
- Trust is per-invocation and explicit, or it comes from the user's own
  `[trust]` list (item 5). Never from `.config/git.tpl.toml` — the project
  cannot consent on the reader's behalf.
- Tasks run on `init`, not on `update`. `update` being a ref-only operation is
  most of its value.
- The rendered commands are shown in full before they run.
- A failure is loud and leaves the ref and the merge intact.

The bar is that the bootstrap script has to be demonstrably worse in practice,
not merely less tidy. It is closer than it looks.


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
stays until it actually hurts. Inheritance (item 7) is what would make it
hurt: a three-deep chain is three clones per run.

**A remote data source is fetched on every run.** Once per run, but never
cached between them — for the same reason the template itself is cloned fresh:
a stale cache silently rendering old data is a far worse failure than a slow
fetch, in a tool whose whole premise is reproducibility. `sha256` is the answer
for anyone who needs the bytes to be the same ones.

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
   HTTP outside `src/data/` — the `http-isolation` prek hook. A template can
   declare a URL; it cannot construct a request.
