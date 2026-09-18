# 0005. The workspace is `~/.kaibo/`

**Status** Accepted
**Decided** 2026-07-03 or earlier (the record carries no separate date)

## Context

Everything that reads the corpus - skills first, later the binary - needs one
predictable place for the clone and for anything else kaibo keeps on a machine.

## Decision

The workspace is `~/.kaibo/`, and the knowledge monorepo clone lives at
`~/.kaibo/knowledge`. No plugin `userConfig`: the path is carried by the tool,
not supplied per installation.

## Consequences

- One known location to inspect, clear or back up, and one answer to "where is
  my corpus" that does not depend on how kaibo was installed.
- The clone target is fixed rather than passed in at invocation time, which is
  the shape [0007](0007-read-content-is-untrusted-data.md) later generalises
  into a rule: targets come from configuration, never from content.
