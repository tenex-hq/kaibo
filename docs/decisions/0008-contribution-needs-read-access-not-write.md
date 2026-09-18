# 0008. Contribution needs read access, not write

**Status** Accepted
**Decided** 2026-08-27

## Context

The obvious way to let somebody contribute knowledge is to grant them write on
the knowledge repo. That hands every contributor a direct push path to the
corpus everyone else's answers are retrieved from, in exchange for the ability
to open a pull request - which does not require write access at all.

## Decision

`contribute` picks its push route from the contributor's own permission on the
target repo:

- with write access, it pushes a branch to the repo;
- otherwise it forks under the contributor's own account and opens a cross-repo
  pull request.

In both cases the fork's parent is verified against the configured target
before any push. New contributors are given read access; write stays with the
owners who merge.

## Consequences

- Onboarding a contributor is a read grant, and nothing about the contribution
  path changes when they get one.
- The blast radius of a compromised contributor account is an open pull
  request, not a commit on the default branch.
- The parent check is what keeps the fork route from being redirected: a fork
  whose parent is not the configured target is not pushed to. It is the same
  configuration-over-content rule as
  [0007](0007-read-content-is-untrusted-data.md), applied to the write side.
- Merge is the ratification step. What reviews a knowledge PR for correctness,
  staleness and contradiction is still open - see [parked.md](parked.md).
