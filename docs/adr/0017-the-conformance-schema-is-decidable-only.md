---
status: accepted
---

# 0017. The conformance schema is decidable only

**Decided** 2026-09-21

Supersedes decision 10 of the conformance-contracts epic
([#28](https://github.com/tenex-hq/kaibo/issues/28)), which read "measure the
judgment half from the first slice" and explicitly rejected a decidable-only
first slice.

## Context

The normative schema was specified with two check species. **Decidable**
checks - four regex-shaped operations - are evaluated by the client and
produce facts: rule id, offset, matched text, no inference. **Judgment** items
execute nothing; they compile the rule's prose into a rubric item carrying a
criterion, allowed verdict values, locators and a response schema, and a model
answers them.

The judgment half was the part harvested from a prototype and the part no
competitor ships, which is exactly why the epic refused to defer measuring it.
So it was measured first, as [#33](https://github.com/tenex-hq/kaibo/issues/33),
before the write-back and the hook that only exist to collect its verdicts were
built.

[`evals/h2`](../../evals/h2/README.md) ran two arms over one hand-fixed set of
five standards and fourteen planted defects, k=3, blind-graded, with the rubric
author, the defect planter and the grader each isolated from the others' work.
The truth set was committed before either arm ran.

| model | prose recall | rubric recall | paired B-A | cost ratio |
|---|---|---|---|---|
| `claude-sonnet-5` | 0.72 | 0.67 | -0.049 (sd 0.330) | 2.6x |
| `claude-haiku-4-5` | 0.59 | 0.59 | +0.000 (sd 0.207) | 3.3x |

A second reading of the same runs measured **closure** - whether every standard
was addressed at all. The rubric dropped nothing across 240 rule-slots; prose
silently skipped 30% of the rule set on the cheaper model. Both arms still
found the same defects. Perfect closure bought zero extra defects.

## Decision

The schema ships with the decidable species only. `"kind": "judgment"` is not
in the vocabulary, and a page that writes one gets a schema error rather than
having the item silently discarded.

Three things follow, and are part of this decision:

- The judgment half comes out of the lint gate and out of the `Stop`-hook
  continuation, which existed solely to collect judgment verdicts.
- Closure stops being argued as a quality improvement. It is real, and it is
  governance: it earns its 3.3x only once something downstream *consumes* a
  dropped verdict - a re-ask, a blocked merge, a count a human acts on. Naming
  that consumer is a product call, tracked as
  [#75](https://github.com/tenex-hq/kaibo/issues/75), and no further arms
  settle it.
- If judgment is reintroduced, locators are applied **per rule**, not compiled
  uniformly across a standard set. The per-rule signature replicated across
  both model tiers to two decimals: the rubric won where the evidence is local
  to a test name (+0.33 and +0.50 on the same rule) and lost exactly half of
  the rule whose evidence needs the whole artifact, on both. Averaged over a
  set, those cancel, which is the only reason the totals look like a tie.

## Consequences

- The schema is additive, backward compatible, and needs no model to be
  useful. A corpus that adopts it gets facts on day one, offline, with the
  decidable half alone.
- Nothing is built for a component that has not earned it. The write-back, the
  hook continuation and the rubric compiler are all unbuilt rather than built
  and later removed, which was the whole point of running the test at step 2 of
  the epic instead of step 5.
- The differentiated component is unshipped, and the first slice looks more
  like a linter than like a judge. That is the honest position: enforced on the
  decidable half, and nothing claimed about a half that measured flat.
- Reopening this needs an experiment, not an argument. The bar is a rubric that
  beats prose on recall at a cost ratio the evidence above does not already
  refute, or a named consumer that makes closure worth paying for on its own.
