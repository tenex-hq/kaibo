# H2: does the compiled rubric beat the same standards as prose?

Issue [#33](https://github.com/tenex-hq/kaibo/issues/33), part of the
conformance-contracts epic [#28](https://github.com/tenex-hq/kaibo/issues/28).

The judgment half of a conformance contract - criterion, `verdict_values`,
locators, `response_schema` - is the component harvested from the `the-arbiter`
prototype and the one no competitor ships. It has never been tested. The
prototype structurally could not test it: comparing two model runs was exactly
what its zero-cost constraint forbade.

**Hypothesis.** Given the same standards, a compiled rubric gets a better answer
out of the caller's model than the same standards as prose.

**Kill condition.** If the prose arm finds the same defects at comparable cost,
the rubric is ceremony. The judgment half of the schema should then be deleted
and the product becomes `query` plus decidable checks - a smaller, clearer
thing. That removes `judgment` checks from the normative frontmatter schema
(#29), shrinks the service-side contract to its decidable half, and removes the
continuation from the service-side `Stop` hook entirely.

This runs **before** #29 lands deliberately. Its kill condition deletes work
from #29, #30 and the service issues, so running it late means building a
write-back, a hook and a continuation for a component that may not earn them.

## Why it is not part of the test suite

Same reason as the rest of `evals/`: it costs real money and talks to a live
model. Nothing here runs in CI. See [`../README.md`](../README.md).

## The trap this design exists to avoid

The naive version of this test bundles two separate claims:

1. **closure beats retrieval** - a contract enumerates the complete binding set,
   a query returns top-k. Almost certainly true, and already the epic's premise.
2. **rubric structure beats prose, holding the rule set fixed** - the actual
   open question.

If the two arms receive different standards, the result measures discovery and
says nothing about the rubric. So **both arms are handed the identical standard
set**, fixed by hand in [`standards/standards.md`](standards/standards.md).
Retrieval never chooses.

## The design

| | arm A (control) | arm B (treatment) |
|---|---|---|
| input | the standards' page text, verbatim, plus "review this artifact against these standards" | the same five standards compiled - criterion, `ask`, `verdict_values`, locators, `response_schema` |
| output shape | free | the schema the rubric names |
| model, artifacts, turn budget, k | identical | identical |

**Scope: judgment items only.** Decidable checks are facts produced by a regex;
comparing them against prose is meaningless. All five standards are judgment
species, and the artifacts carry defects of *taste*, not of mechanism.

**No temperature control.** `claude -p` does not expose it. The requirement the
design actually needs is that both arms run at the *same* temperature, which
holds; what is lost is the ability to pin it. Variance is therefore measured at
whatever the default is, which is also what production sees.

### Who authored what, and why that matters

The prototype's calibration lesson stands: a self-authored truth set scored 1.00
precision and 1.00 recall on one prototype and 0.00 once an adversary touched
the input. So the roles are split and the isolation is enforced by what each
author was allowed to read.

| role | reads | never reads |
|---|---|---|
| rubric author | `standards/`, `docs/concept/testing.md` | `artifacts/`, `truth/` |
| defect planter | `standards/`, `artifacts/clean/` | `arms/rubric.json` |
| grader | `truth/`, normalised findings | which arm produced a finding |

The truth set is **committed before either arm runs**. Its commit is the
paper trail; a truth set edited after seeing a run is not a truth set.

### Blind grading

Arm B answers in JSON with rule ids; arm A answers in prose. A grader reading
raw output can tell the arms apart at a glance, so grading happens in two
passes: a **normaliser** rewrites every answer from both arms into one common
finding shape, then the **grader** sees those findings shuffled together, with
the arm label stripped, and matches them to the truth set.

Recall is credited on what a finding *describes* - the defect and its location -
not on whether it cites the right rule id. Arm A is not handed ids, so scoring
attribution would measure the harness, not the arms. Rule-id attribution is
recorded separately as an observation.

## Metrics

Per standard, per arm:

- **recall** - did it identify the hand-enumerated defect
- **false positives** - did it flag something in a span the truth set records as
  clean
- **variance across repeats** - k >= 3. A rubric that wins on average but swings
  wildly is not a gate. A single-run comparison of two prompts is noise, and
  treating it as a result is how this test produces a confident wrong answer.
- **token cost** - the rubric ships spans and schema, so it is not free. The
  prototype returned 15 rubric items across 6 drafts with only 5 real defects
  behind them, and 5 of 11 items on its own docs fired on vocabulary rather than
  behaviour. Cost is a column, not a footnote.

## Layout

```
standards/standards.md   the five standards, verbatim. Both arms get these.
artifacts/clean/         real kaibo test excerpts, copied verbatim
artifacts/under-test/    the same excerpts with defects planted
truth/truth-set.json     the hand-enumerated defects, committed before any run
arms/rubric.json         the compiled contract (arm B)
run.py                   drives both arms
runs/<timestamp>/        raw model output, usage and cost per call
```

## Running it

Free, and the thing to do first - it prints both arms' prompts in full:

```bash
python3 evals/h2/run.py --dry-run
```

Costs money:

```bash
python3 evals/h2/run.py --model claude-sonnet-5 --k 3
```

Each call runs with `--safe-mode --tools ""`, so no `CLAUDE.md`, no skills, no
plugins, no hooks, no MCP and no filesystem. The artifact is in the prompt. That
is what makes a run reproducible from the manifest alone.

## Verdict

Not run yet.
