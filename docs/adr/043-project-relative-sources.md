# ADR-043: Configured local sources are project-relative

## Status

Accepted.

## Decision

Resolve a CLI template path from the invocation directory, then record it relative to the destination project root.
Resolve configured paths and rendered `Template-Source` provenance from that project root in every consumer,
including status, previews, log and backport.
Keep the configured spelling, filesystem location and ref identity separate.
When rebasing changes the derived identity, persist the original identity in the existing `template.id` field.
URLs and scp-style addresses are unchanged; manifest-authored dependencies retain their existing semantics.
Missing local paths fail as local repository access, without remote fallback.

## Compatibility

This changes the interpretation of old cwd-relative configuration and provenance.
There is no reliable way to infer the directory from which an old command ran.
Users must correct ambiguous old paths relative to the project root and retain the existing ref ID explicitly.
No automatic migration guesses a location or creates a replacement rendered history.
