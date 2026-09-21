# H2: does the compiled rubric beat the same standards as prose?

[#33](https://github.com/tenex-hq/kaibo/issues/33), part of the
conformance-contracts epic [#28](https://github.com/tenex-hq/kaibo/issues/28).

> **Closed.** Ran 2026-09-20 on two models. It does not. Verdict in
> [ADR 0017](../../docs/adr/0017-the-conformance-schema-is-decidable-only.md).
> `run.py` is here to be read, not re-run: `runs/` cannot be regenerated
> (`claude -p` pins no temperature) and the question has an answer.

Both arms get the identical five standards, fixed by hand. Arm A gets the page
text; arm B gets it compiled into a rubric. k=3, 14 planted defects, 3 decoys.

## Result

| model | arm | recall | precision | cost |
|---|---|---|---|---|
| `claude-sonnet-5` | A prose | **0.72** | 0.83 | $0.56 |
| `claude-sonnet-5` | B rubric | 0.67 | 0.87 | $1.43 |
| `claude-haiku-4-5` | A prose | 0.59 | 0.86 | $0.30 |
| `claude-haiku-4-5` | B rubric | 0.59 | 0.93 | $0.97 |

Paired by (artifact, repeat): sonnet -0.049 (sd 0.330), haiku +0.000 (sd
0.207). No measurable recall difference, at 2.6x and 3.3x. That is the kill
condition as written.

## The effect is per rule, not per product

| rule | sonnet A / B | haiku A / B |
|---|---|---|
| t1 expected value from code under test | 1.00 / 0.83 | 1.00 / 0.83 |
| t2 can fail for the reason its name gives | 0.33 / 0.27 | 0.20 / 0.27 |
| t3 situation, not a function call | 0.67 / **1.00** | 0.33 / **0.83** |
| t4 hermetic | 1.00 / 1.00 | 0.89 / 0.89 |
| t5 sweep, do not enumerate | **1.00** / 0.50 | **1.00** / 0.50 |

Three rules agree across model tiers to two decimals, so the effect is a
property of the rubric, not of the model reading it. A rubric **aims** a model:
it wins where the evidence is local (t3, a test name) and loses where the rule
needs the whole artifact in view (t5, exactly half on both). Averaged over a
rule set those cancel, which is why the totals look like a tie.

## Closure (#75 baseline)

Which standards an arm addressed at all, read out of the same answers.

| model | arm | addressed | silently absent, of 60 |
|---|---|---|---|
| `claude-sonnet-5` | A prose | 0.95 | 3 |
| `claude-sonnet-5` | B rubric | **1.00** | 0 |
| `claude-haiku-4-5` | A prose | 0.70 | **18** |
| `claude-haiku-4-5` | B rubric | **1.00** | 0 |

Real and large - 30% of the rule set silently skipped on the weak model - and
**inert**. Both haiku arms scored 0.59: prose was not missing defects on the
rules it skipped, it was skipping rules with no defects in them. Closure is a
governance argument, not a quality one.

## Method

The isolation is the experiment. A self-authored truth set scored 1.00/1.00 on
the prototype this inherits from, and 0.00 once an adversary touched the input.

| role | reads | never reads |
|---|---|---|
| rubric author | `standards/` | `artifacts/`, `truth/` |
| defect planter | `standards/`, `artifacts/clean/` | `arms/rubric.json` |
| grader | `truth/`, normalised findings | which arm produced a finding |

The truth set is committed **before** either arm runs; its commit is the paper
trail. Arm B answers JSON and arm A prose, so a normaliser rewrites both into
one finding shape before the grader sees them shuffled and unlabelled. Recall
is credited on what a finding describes, not on rule-id attribution, which arm
A is never given. Each call runs `--safe-mode --tools ""`: no skills, hooks,
MCP or filesystem, so a run is reproducible from its manifest alone.

## Layout

```
standards/standards.md   the five standards. Both arms get these.
artifacts/clean/         excerpts from this repo's own tests, verbatim
artifacts/under-test/    the same excerpts with defects planted
truth/truth-set.json     the defects, committed before any run
arms/rubric.json         the compiled contract (arm B)
run.py, grade.py, closure.py
runs/<timestamp>/        raw output, usage, cost, gradings, report
```

Not part of the test suite: it costs money and talks to a live model. See
[`../README.md`](../README.md).

## What this does not establish

- Two Anthropic models, 14 defects, k=3, one grader. Another vendor or a much
  longer artifact may differ.
- **t2 is 5 of the 14 defects and both arms are bad at it.** Read per-rule
  recall, not the totals.
- One author's rubric, written blind and machine-checked against the standards
  text. That is the best available guard, not a proof.
- Rust test code judged against testing standards. Another domain may differ.
