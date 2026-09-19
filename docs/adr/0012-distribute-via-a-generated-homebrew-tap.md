---
status: accepted
---

# 0012. Distribution is a generated Homebrew tap, not an install script

**Decided** 2026-09 (the record carries no exact date)

## Context

The default way to ship a developer CLI is a hand-written `install.sh` behind a
curl one-liner: another artefact to maintain, to keep in step with the release,
and for every user to trust.

## Decision

The release tooling emits distribution from the tag. cargo-dist with
`installers = ["shell", "homebrew"]` produces the release artefacts, the curl
one-liner and the Homebrew formula from one tag. The tap is the public
`tenex-hq/homebrew-tap`.

## Consequences

- Installer and formula are generated from the same tag, so they cannot
  disagree about a version, and there is no install script to hand-maintain or
  hand-audit.
- `brew upgrade` becomes the update path, which is what makes
  [0013](0013-ship-the-skills-inside-the-binary.md) - prose and mechanism
  updating atomically - possible at all.
- The tap does not constrain where release assets live, so it does not pin any
  other repository decision.
