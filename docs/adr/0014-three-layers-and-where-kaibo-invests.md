---
status: accepted
---

# 0014. Three layers, and where kaibo invests

**Decided** 2026-09-19

## Context

Asked directly what makes kaibo better than retrieval-augmented generation
over the same corpus, the honest first answer is: on the retrieval itself,
nothing. kaibo embeds markdown, searches it semantically, and returns ranked
passages with citations. That is the textbook definition of the retrieval half
of RAG, and the work is done by `qmd`, not by kaibo. Strip the CLI away and a
bare `qmd query` against the same index gets most of the way. Hosted products
do this too, several with more engineering behind them.

The question matters because it decides where effort goes. A tool that cannot
say which part of itself is differentiated will spend its time improving the
part that is not.

Separating the answer into layers makes the investment case decidable, and
makes it survivable when the retrieval stack is replaced.

## Decision

kaibo is three layers, and they are funded differently.

| Layer | What it is | Posture |
|---|---|---|
| L1 Retrieval | embeddings, ranking, chunking, hybrid search | **Do not invest.** Commodity, and deliberately replaceable. |
| L2 Corpus quality | what is written, how atomic it is, whether it is still true | **Invest.** Mostly not code. |
| L3 Consumption trigger | whether an agent loads doctrine at the moment it matters | **Invest.** Highest ceiling, still unsolved. |

**L1 stays boring on purpose.** Retrieval is bought, not built, and `qmd` is
swappable by design ([0006](0006-dedicated-qmd-index.md),
[0010](0010-the-default-index-has-no-exceptions.md)). Effort spent making kaibo
a better retriever is effort spent on the half with no moat. Ranking quality is
not a kaibo problem.

**L2 is the actual product.** Classic RAG points at whatever exists: the wiki
nobody has updated in two years, exported chat logs, a document graveyard.
kaibo's corpus arrives by pull request, through `contribute`, under CODEOWNERS
review and a lint gate, and it is prescriptive rather than descriptive: it
records what the organisation decided should be true, not what happened to be
written down. What kaibo competes against is not "RAG over our docs," it is
"our docs being worth retrieving at all." That is an editorial product wearing
a CLI, and the highest-leverage work in it is frequently writing pages rather
than writing code.

**L3 is where the hard problem lives.** A corpus nobody loads at the right
moment is worth nothing however good it is. Strengthening prose in skill
descriptions to force triggering has been tried and did not work: an agent
skipped kaibo, then on challenge recited the rule correctly, because post-hoc
articulation is not pre-hoc triggering. Rules keyed on a felt knowledge gap
are unfollowable, since a model with a repo in front of it does not classify
its own answer as general knowledge. Rules keyed on task entry are followable
([0009](0009-agents-consume-kaibo-on-task-entry.md)). Any further intervention
here must be measured, not argued.

**Measurement precedes intervention at L3.** Without an activation number,
every trigger change is unfalsifiable and the failure mode repeats. The
binary's own paper trail, and whatever reads it, are therefore a prerequisite
for L3 work rather than a nice-to-have.

## Consequences

- Investment order follows the layers, not the backlog: measurement of L3
  first, then the L2 work that pays off under every branch, then L3
  interventions the measurement can now judge.
- **Atomicity enforcement is decoupled from conformance contracts.** An atomic
  corpus retrieves better, cites cleanly and composes into proposals without
  contradiction, whether or not a `judge` verb ever exists. It is funded as L2
  work in its own right.
- **Page-level staleness is a gap.** `status` detects a stale clone; nothing
  detects a doctrine page that quietly stopped being true. A wrong page cited
  with confidence is worse than a missing one, which makes this L2 and not
  cosmetic.
- **Honest gaps are defended as a differentiator.** Answering "the
  organisation has no position on this, the nearest domain is X" is behaviour
  a generic RAG endpoint does not have, it is cheap to sharpen, and it is
  protected rather than traded away for coverage
  ([0003](0003-grounded-answers-and-labeled-proposals.md)).
- **New verbs are presumed harmful.** Every added tool degrades selection
  across the whole set. A capability must argue its way in against that cost,
  which is the reasoning that folded `advise` into `query`.
- kaibo never becomes the store ([0002](0002-ai-optional-core.md)).
  Orchestrating git, qmd and markdown is the shape, and L1 being commodity is
  what keeps that true.
- Conformance contracts are assessed by which layer they serve. Their durable
  idea is L3: keying retrieval on the action the caller is taking removes the
  agent's need to suspect a gap first. Their cost is a schema and a rubric
  whose value is unproven, which is why that half is settled by experiment
  before it is built.
