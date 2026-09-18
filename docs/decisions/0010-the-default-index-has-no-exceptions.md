# 0010. The default qmd index has no exceptions

**Status** Accepted
**Decided** 2026-08-30

## Context

`sync` carried a one-time migration guard that removed two pre-monorepo
collections from qmd's *default* index, for machines that had indexed the
corpus before [0006](0006-dedicated-qmd-index.md). It was the only step in
kaibo that named the default index at all, which made it the sole exception to
an otherwise unconditional invariant. Eight weeks after the migration, and
verified already gone on the machine it existed for, it was carving out an
exception nothing needed.

## Decision

Delete the carve-out. Only the configured index is ever written; the default
index is not a target under any condition. `qmd collection remove` left the
skill's `allowed-tools` with it.

## Consequences

- The invariant is unconditional, so it can be enforced by a test that sweeps
  the source rather than by a test plus a list of permitted exceptions - and a
  sweeping test catches the next code path that tries, which an enumerated one
  would not.
- A machine that never migrated keeps two stale personal collections. That is a
  cosmetic annoyance, removable by hand, and not worth a permanent exception
  that the invariant then has to keep carving out.
