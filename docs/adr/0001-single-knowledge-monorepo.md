---
status: accepted
---

# 0001. Single knowledge monorepo, one folder per domain

**Decided** 2026-07-07

## Context

The original design distributed the corpus across one repository per domain,
plus an index repo and a `/register` skill that told retrieval which repos
existed. The stated rationale for the split was per-repo write access: a domain
owner should be able to grant write on their domain alone.

Two things falsified it.

The rationale was already nullified by our own workflow. Contribution happens
by pull request, so nobody needs write on a knowledge repo to add a page, and
per-repo write grants buy nothing that ownership metadata does not.

A 2026-07-06 architecture review then showed the split was actively costing.
Nearly every high-severity defect it found was multi-repo tax, and the worst of
them was silent: repos that were never registered made most of the real pages
in the corpus invisible to `query`. The failure mode of a distributed corpus is
not an error, it is an answer that omits what exists.

## Decision

One knowledge monorepo, one folder per domain, with ownership expressed as
CODEOWNERS entries instead of repository boundaries. The index repo and the
`/register` skill are deleted.

## Consequences

- Retrieval covers the whole corpus from a single collection. There is no
  fan-out routing, no registration step, and nothing to forget to register.
- Ownership becomes a file in the repository rather than a permission grant,
  reviewable in a diff.
- Several parked ideas die outright: lazy per-repo materialization, fan-out and
  multi-select routing, cross-cutting domain flags, clone pruning, a unified
  cross-repo index.
- Splitting a folder back out into its own repository later is mechanical, so
  this is a cheap decision to reverse.

## Revisit when

A domain needs read confidentiality - some readers of the corpus must not see
it. That is the un-park trigger for splitting a domain back out. Write access
is not a trigger; CODEOWNERS covers it.
