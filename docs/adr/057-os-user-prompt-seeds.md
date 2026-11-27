# ADR-057: The OS login name may seed an interactive prompt

**Status:** accepted

**Supersedes:** [ADR-018](018-seed-context.md)'s exclusion of `user` from the seed context

**Relates to:** [ADR-006](006-no-runtime-context.md), [ADR-056](056-utc-datetime-prompt-seeds.md)

## Context

A template may ask for a login name rather than the display name stored in Git configuration.
The current OS account supplies a useful suggestion, but acquiring it during rendering would violate determinism.
The prompt-only seed context already allows machine facts to become answers after human confirmation.

## Decision

Add `user` to the closed seed context, available only to `default_from` expressions:

```toml
[questions.username]
type = "string"
default_from = "{{ user.name | default('developer') }}"
```

Initially, the only field is `user.name`: the current OS account's login name.
Use `whoami`'s native account lookup, which reads the effective account on Unix and the current account on Windows.
This is independent of the account's full name and the Git display name available as `git.user.name`.
No shell process is started and no environment-variable approximation of the login is used.

Capture the account name once when the existing lazy seed snapshot is first needed.
Preserve a successful UTF-8, nonblank name verbatim.
Always expose a `user` table, leaving `name` undefined when lookup fails or the name is blank or not valid UTF-8.
This lets `| default(...)` handle unavailable account information without failing the prompt.

All other prompt-only boundaries remain:

- Seeds require interactive answering with a project repository.
- Supplied or reused answers and compatible personal defaults win without acquiring seeds.
- Defaults-only runs do not acquire seeds; stable ordinary defaults or supplied answers remain necessary.
- The seed context is a separate type and is available only to string questions' `default_from` expressions.
- A suggestion becomes an ordinary recorded answer only after human confirmation.
- No account information is injected into rendering, computed values, conditions, or ordinary defaults.

The namespace set is now `git`, `dir`, `remote`, `now`, and `user`; further additions still require an ADR.

## Consequences

Templates can suggest login names without conflating them with Git identity.
Recorded answers remain reproducible across users, and later updates do not reacquire an already answered login.
Standalone rendering without a project repository retains its existing seed behavior.

This is an additive manifest capability, with no persisted configuration or CLI format changes.
Older versions reject the new namespace during manifest validation.
The cross-platform account lookup adds a dependency confined to prompt seed collection.
