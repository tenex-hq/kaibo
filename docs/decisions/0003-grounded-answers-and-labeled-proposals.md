# 0003. Two answer classes: grounded answer and labeled proposal

**Status** Accepted
**Decided** 2026-07-07

## Context

Once retrieval feeds a model, it will answer questions no page covers by
synthesising across the pages that exist. Presented in the same shape as a
retrieved fact, that synthesis is indistinguishable from knowledge. It also
quietly makes the AI layer load-bearing, against
[0002](0002-ai-optional-core.md): the value exists only while the model is in
the loop, and disappears when it is not.

## Decision

`query` distinguishes two classes of answer.

- **Grounded answer** - a retrieved fact, with hard citations to the pages it
  came from.
- **Labeled proposal** - a synthesis across cited primitives, explicitly marked
  ("no page covers this; proposed from [A] + [B]; verify before relying"),
  surfacing its own risk, and never silently built on drafts.

Proposals are ephemeral by design. The flywheel is what converts them:

> proposal -> `contribute` -> owner ratifies -> plain page

What was a proposal becomes a grounded answer, readable at L1 with no AI in the
loop at all.

## Consequences

- The consultant path - asking for advice the corpus does not hold - is
  allowed without breaking the AI-optional invariant, because every proposal
  worth keeping has a documented route to becoming a plain page.
- A proposal nobody ratifies simply expires. That is the intended outcome, not
  a leak.
- This supersedes an earlier plan that made proposals depend on fan-out routing
  across repos; that prerequisite died with the monorepo consolidation, since
  one collection already means corpus-wide retrieval
  ([0001](0001-single-knowledge-monorepo.md)).
