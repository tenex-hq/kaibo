# Handoff: kaibo conformance-contracts, after H2

Written 2026-09-20. Everything below is committed and pushed; nothing lives
only in the session that produced it.

## What was done

Ran issue #33, the H2 test: does a compiled rubric beat the same standards as
prose? It does not. Then read #75's baseline (closure) out of the same runs for
the price of one extraction pass.

## The evidence, in one place

Two arms, identical hand-fixed standard set, 14 defects, k=3, blind grading.

| model | prose recall | rubric recall | paired B-A | cost ratio |
|---|---|---|---|---|
| `claude-sonnet-5` | 0.72 | 0.67 | -0.049 (sd 0.330) | 2.6x |
| `claude-haiku-4-5` | 0.59 | 0.59 | +0.000 (sd 0.207) | 3.3x |

Closure, same runs, standards addressed at all:

| model | prose | rubric | prose slots silently absent, of 60 |
|---|---|---|---|
| `claude-sonnet-5` | 0.95 | 1.00 | 3 |
| `claude-haiku-4-5` | 0.70 | 1.00 | 18 |

Three conclusions, in order of how load-bearing they are:

1. **The rubric does not buy recall**, on either model. #33's kill condition as
   written. Precision favours it slightly (0.87/0.83, 0.93/0.86).
2. **Closure is real and does not convert.** Prose silently skips 30% of the
   rule set on a cheap model; the rubric drops nothing across 240 rule-slots.
   Both arms still found the same defects. Perfect closure bought zero extra
   defects at 3.3x.
3. **The effect is per rule, not per product.** The per-rule signature
   replicates across two model tiers to two decimals: rubric wins t3 (evidence
   local to a test name, +0.33 and +0.50), loses exactly half of t5 on both
   (evidence needs the whole artifact). Averaged over a rule set they cancel,
   which is why the totals look like a tie.

## State of the tracker

| | |
|---|---|
| PR #76 | open, 6 commits, pushed. Closes #33. Needs review or merge. |
| #33 | closed. Verdict and replication posted as comments. |
| #75 | open, narrowed by a comment to one question. See below. |
| #28 | epic, blocked on a product decision rather than on data. |
| #29 | the epic's step 1, unstarted, blocks everything else. |

## The one open question

**What consumes a dropped item?** Closure is worth 3.3x only if a dropped
verdict triggers something: a re-ask, a blocked merge, a count a human acts on.
If nothing reads drops today, closure is a governance feature with no consumer.
This is a product call and cannot be settled by running more arms. Do not spend
a pressured run on it; the baseline is already decisive.

## Recommended next move

**Land #29 decidable-only.** It is the epic's step 1 and everything is behind
it, the evidence already says `judgment` should not be in it, and #75 can only
ever add it back later. It costs nothing whichever way the product call goes.

Per #33's recommendation, also drop the judgment half of #30's gate and the
`Stop`-hook continuation, which exists solely to collect judgment verdicts.
The decidable half is untouched by all of this: it produces facts, needs no
model, and was never what H2 questioned.

If judgment is ever reintroduced, apply locators **per rule** rather than
compiling them uniformly across a standard set.

## Where the artifacts are

`evals/h2/`, not part of the test suite, not in CI, costs real money.

- `standards/standards.md` - the five standards, sha-pinned into the truth set
  and every run manifest
- `truth/truth-set.json` - 14 defects, 3 decoys, committed before any arm ran
- `arms/rubric.json` - compiled from the standards text alone
- `run.py`, `grade.py`, `closure.py` - arms, blind grading, closure
- `runs/20260920T102010Z` (sonnet), `runs/20260920T110157Z` (haiku) - raw
  answers, manifests, gradings, reports
- `README.md` - full verdict, replication, closure, and what none of it
  establishes

## Traps for whoever picks this up

- **The isolation is the experiment.** Rubric author never saw an artifact;
  planter never saw the rubric; grader never saw which arm produced a finding.
  Re-running with those collapsed produces a number that means nothing. The
  prototype this inherits from scored 1.00/1.00 self-authored and 0.00 once an
  adversary touched the input.
- **A truth set edited after seeing a run is not a truth set.** It is committed
  ahead of the run commit on purpose.
- **t2 carries 5 of the 14 defects and both arms are poor at it.** Read
  per-rule recall, not the totals.
- **`run.py` piped through `tail` looks stalled** - output buffers until exit.
  Watch `runs/<stamp>/raw/` for progress instead.
- Two deviations from #33 as written, both documented in the README:
  `claude -p` cannot pin temperature, and recall is credited on what a finding
  describes rather than on rule-id attribution.

## Not done, deliberately

No eval-methodology page was contributed to the corpus. It was offered and
declined; the method lives in `evals/h2/README.md` instead.
