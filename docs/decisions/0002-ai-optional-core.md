# 0002. AI-optional core, with an explicit degradation ladder

**Status** Accepted
**Decided** 2026-07-07

## Context

Semantic search and model synthesis are the obvious selling points of a
knowledge tool, and the obvious single point of failure. If either becomes
load-bearing, the corpus stops being readable the day the AI layer is
unavailable, degraded, rate-limited or unaffordable - and a knowledge base you
cannot read on a bad day is not a knowledge base.

## Decision

The corpus must be fully usable with eyes, grep and BM25 alone. AI layers are
accelerators, never load-bearing. The degradation ladder is explicit:

| Level | Capability |
|---|---|
| L0 | folder structure and `_index.md` - navigable by reading the tree |
| L1 | grep, code search, or an editor over the clone (it opens as a plain markdown vault) |
| L2 | `qmd search` / `vsearch` - retrieval with no LLM |
| L3 | `qmd query` - local-LLM retrieval |
| L4 | `kaibo query` grounded answers |
| L5 | labeled proposals |

Every level must stay useful if all levels above it vanished.

## Consequences

- Content conventions are load-bearing, not cosmetic. Question-shaped titles,
  honest tags and atomic pages are what keep L1 and L2 alive, which is why they
  are linted rather than suggested.
- The CLI orchestrates git, qmd and markdown and never becomes the store. A
  corpus only a binary can read would be a different product.
- `query` retrieves and does not synthesise: no model call, no API key in the
  binary. `--explain` prints the underlying commands so the lower rungs stay
  reachable when the binary is absent.
- Value produced at L4 and L5 does not persist by itself, so it has to be
  converted into L0-level content. That conversion is
  [0003](0003-grounded-answers-and-labeled-proposals.md).
