# 0011. Rust, not Go

**Status** Accepted
**Decided** 2026-09 (the record carries no exact date)

## Context

The orchestration layer - git, qmd and markdown, previously carried as skill
prose - was moving into a binary that ships to developer machines. Rust and Go
were the two candidates.

## Decision

Rust.

The two languages were judged **equally capable** for this workload, and the
tiebreaker was circumstantial: there was an in-house worked example of exactly
this publishing pipeline, a release build feeding a Homebrew tap, already
written in Rust. That example is itself an unreviewed draft, so it is a
tiebreaker and not evidence.

## Consequences

- No claim is made that Rust is the better language for this tool, and none
  should be inferred from the choice. Had the worked example been in Go, the
  decision would plausibly have gone the other way.
- What the decision actually buys is a publishing path somebody has already
  walked end to end: see [0012](0012-distribute-via-a-generated-homebrew-tap.md).
