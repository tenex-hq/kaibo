# H2: does the compiled rubric beat the same standards as prose?

Issue [#33](https://github.com/tenex-hq/kaibo/issues/33), part of the
conformance-contracts epic [#28](https://github.com/tenex-hq/kaibo/issues/28).

> **Closed. This is a record, not a harness.**
>
> The experiment ran on 2026-09-20, on two model tiers, and the kill condition
> below fired: the rubric bought no recall at 2.6x to 3.3x. The verdict is
> [ADR 0017](../../docs/adr/0017-the-conformance-schema-is-decidable-only.md),
> and `kind: judgment` is not in the schema.
>
> `run.py` is here so the arms can be read, not re-run. It costs real money and
> the question it asks already has an answer - and `runs/` cannot be
> regenerated anyway, since `claude -p` pins no temperature. Reopening this
> takes a *new* experiment measured against these numbers, not this one again.
>
> What is still live: the isolation protocol and the truth-set discipline, which
> any future H-test should reuse. See [The trap this design exists to
> avoid](#the-trap-this-design-exists-to-avoid) and [The design](#the-design).

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

**The hypothesis does not hold. The rubric does not buy recall, and it costs
2.6x to 3.3x.** Tested on two models; neither shows a recall difference. The
primary run below is `runs/20260920T102010Z`, `claude-sonnet-5`, k=3, 14
defects, 24 attempts, no harness errors. The replication on
`claude-haiku-4-5-20251001` follows.

| arm | recall | false positives per attempt | decoy hits | precision | cost |
|---|---|---|---|---|---|
| A, prose | **0.72** | 0.50 | 1 | 0.83 | $0.56 |
| B, rubric | 0.67 | 0.33 | 0 | 0.87 | $1.43 |

Paired by (artifact, repeat), the rubric is 0.049 *behind* on recall, sd 0.330
over 12 pairs: B wins 2, ties 6, A wins 4. That is not a rubric that loses, it
is a difference the experiment cannot resolve from noise. The honest reading is
**no measurable difference in recall, at 2.6x the price**.

This is the kill condition as written: "if the prose arm finds the same defects
at comparable cost, the rubric is ceremony." The prose arm found the same
defects at *lower* cost.

### What the rubric did buy

Not nothing, and not what the hypothesis claimed:

- **Precision 0.87 against 0.83**, and it flagged none of the three decoys while
  the prose arm flagged one. The rubric's "what does NOT count" carve-outs
  appear to do their job.
- **Closure.** Every standard came back with an explicit verdict, so a dropped
  item is detectable. The prose arm simply omits what it did not consider, and
  nothing distinguishes that from a clean bill.

Closure is the epic's "single most valuable change here", and **H2 did not test
it**. Recall was the claim on trial. A verdict-accounting mechanism that costs
2.6x and finds no more defects is a different proposition from a rubric that
reads better, and it should be argued on its own terms rather than rescued by
this result.

### Where the two arms actually disagreed

Per-rule recall, which matters because the defects are not evenly spread:

| rule | defects | arm A | arm B |
|---|---|---|---|
| t1 expected value from code under test | 2 | 1.00 | 0.83 |
| t2 can fail for the reason its name gives | 5 | 0.33 | 0.27 |
| t3 situation, not a function call | 2 | 0.67 | **1.00** |
| t4 hermetic | 3 | 1.00 | 1.00 |
| t5 sweep, do not enumerate | 2 | **1.00** | 0.50 |

Two splits carry almost the whole difference and they point opposite ways. The
rubric took t3 3/3 where prose managed 1/3: naming quality is exactly the kind
of rule a locator ("every `#[test]` function's name") aims attention at. It lost
t5 entirely on `a2-d1`, the pre-existing guardrail that enumerates five attribute
names, which prose caught 3/3.

**The hermeticity pair worked as designed.** `a4-d2` is a real `std::env::var`
violation and `a1-x1` is the opt-in env-gated exception t4 carves out in its own
text; the two are lexically near-identical. Both arms called the violation 3/3
and neither called the exception. Neither arm was pattern-matching on
vocabulary, which is the most reassuring single fact in the run.

## Replicated on a second model

Run `20260920T110157Z`, `claude-haiku-4-5-20251001`, same truth set, same
rubric, same k, same grader.

| arm | recall | FP per attempt | precision | cost |
|---|---|---|---|---|
| A, prose | 0.59 | 0.33 | 0.86 | $0.30 |
| B, rubric | 0.59 | 0.17 | 0.93 | $0.97 |

Paired, the difference is **+0.000**, sd 0.207: B wins 3, ties 6, A wins 3. An
exact tie, at 3.3x the cost.

Haiku was chosen over a second strong model deliberately. A rubric is an
instruction competing for a model's attention, so the strongest remaining case
for it is that structure helps most where unaided reasoning is weakest. It does
not. Both models land on no recall difference, and the cheaper model pays a
*higher* multiple for it.

### The per-rule signature replicates, which is the real finding

| rule | sonnet A / B | haiku A / B |
|---|---|---|
| t1 expected value from code under test | 1.00 / 0.83 | 1.00 / 0.83 |
| t2 can fail for the reason its name gives | 0.33 / 0.27 | 0.20 / 0.27 |
| t3 situation, not a function call | 0.67 / **1.00** | 0.33 / **0.83** |
| t4 hermetic | 1.00 / 1.00 | 0.89 / 0.89 |
| t5 sweep, do not enumerate | **1.00** / 0.50 | **1.00** / 0.50 |

Three rules give the same answer on both models to two decimal places, including
t5 at exactly 1.00 against 0.50 twice. That is not two runs agreeing by luck: the
effect is a property of the rubric, not of the model that read it.

So the rubric is not uniformly neutral. It has a shape:

- **It helps where attention is the problem.** t3 is judged entirely from test
  function names, and a locator saying "every `#[test]` function's name" is
  exactly the instruction a model reading prose forgets to follow. This is the
  rubric's largest and most reliable win, and it is bigger on the weaker model
  (+0.50) than on the stronger one (+0.33).
- **It hurts where the rule needs a whole-artifact argument.** t5 asks whether a
  guardrail enumerates rather than sweeps, which requires holding the entire
  test in view and asking what it would miss. Both models lost exactly half of
  t5 under the rubric. Decomposing that rule into located spans appears to break
  the judgement it requires.
- **It consistently buys precision**: 0.87 against 0.83, and 0.93 against 0.86.

The honest summary is that a rubric is a way of *aiming* a model, and aiming is
not free. It buys recall on rules whose evidence is local and costs recall on
rules whose evidence is global. Averaged over a rule set, those cancel, which is
precisely what both runs show.

### What this does not establish

- **Two models, 14 defects, k=3.** Both are Anthropic models read by one
  grader. A different vendor, or a much longer artifact, may behave differently.
- **t2 is 5 of 14 defects and both arms are bad at it** (0.33 and 0.27). The
  aggregate is partly a statement about t2. Three of its five defects argue from
  confounding rather than from "this assertion can never fail", and both arms
  missed `a3-d2` and `a3-d4` 0/3.
- **The rubric is one author's rubric.** A stronger one may exist. It was written
  blind and machine-checked against the standards text, which is the best
  available guard, not a proof.
- **Artifacts are Rust test code judged against testing standards.** Another
  domain may behave differently.

## Closure, measured on the same two runs (#75 baseline)

H2 measured which defects an arm found. Closure is a different property of the
same answers: which of the five standards an arm *addressed at all*, whether it
found a problem or not. `closure.py` reads it out of the raw answers already on
disk, so the unpressured baseline cost one extraction pass and no new arms.

| model | arm | standards addressed | silently absent, of 60 |
|---|---|---|---|
| sonnet-5 | A prose | 0.95 | 3 |
| sonnet-5 | B rubric | **1.00** | 0 |
| haiku-4-5 | A prose | 0.70 | **18** |
| haiku-4-5 | B rubric | **1.00** | 0 |

**Closure is real, and it is large where it matters.** On the weaker model the
prose arm silently skipped 30% of the standard set: 18 of 60 rule-slots got
neither a violation nor a clearance, and nothing in the output distinguishes
that from a clean bill. The rubric arm dropped nothing, on either model, across
240 rule-slots.

**And it did not convert into a better answer.** On haiku both arms scored
exactly 0.59 recall. The rubric considered every rule and the prose arm
considered 70% of them, and they found the same number of defects. The prose arm
was not missing defects on the rules it skipped; it was skipping rules that had
no defects to find, and spending its attention on the ones that did.

That is the finding #75 exists to establish, and it cuts both ways:

- The auditability claim in [#28](https://github.com/tenex-hq/kaibo/issues/28) is
  **true**. "The server issued 12 items, 9 came back, 3 were dropped" is
  detectable, and on a cheap model the drop rate is 30%.
- The claim that detectability *improves the outcome* is **unsupported**. Perfect
  closure bought zero additional defects at 3.3x the cost.

So closure is worth paying for only if the dropped item itself is the product -
an audit trail, a compliance record, a count someone acts on - and not if the
goal is finding more problems. That is a governance argument, not a quality one,
and it should be made as such.

### Recommendation

Do not build the judgment half on the strength of H2. Specifically: drop
`judgment` checks from #29's schema, drop the judgment half of #30's lint gate,
and drop the `Stop`-hook continuation, which exists only to collect judgment
verdicts and is the friction the epic already flagged as revisitable. The
decidable half is untouched by this result: it produces facts, needs no model,
and was never what H2 questioned.

If the judgment half is kept anyway, keep it for closure and argue *that*, with
its own measurement. Do not cite H2 as support.
