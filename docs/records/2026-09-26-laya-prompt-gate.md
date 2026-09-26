---
title: "Prompt gate: zero-shot Laya as a nag for the query trigger"
---

# 2026-09-26: Prompt gate, zero-shot Laya as a nag for the query trigger

Whether a local `UserPromptSubmit` hook that classifies each prompt, and injects
one line telling the agent to run `kaibo:query` when it is confident, beats the
skill's `description` alone. The classifier under test was
[Laya](https://github.com/NandhaKishorM/laya) 0.3.5, the English base
checkpoint, zero-shot. The method, the labelled set and the bar were all fixed
before anything was scored. The experiment's scripts and labelled set were not
kept; this record is what remains.

**Verdict: do not ship.** Zero-shot Laya is rejected outright. A 33M-parameter
embedding model did as well with under a tenth of the weights. A gate of any kind
needs a supervised head trained on labels at the real trigger rate before a
hook is worth building.

## What happened

All runs were on an Apple M3 Pro (12 cores, 36 GB) that other agents were
using at the same time, with torch 2.14.0, transformers 5.17.0 and
sentence-transformers 6.1.0 under `uv run`. Nothing was installed globally.

### Latency

| model | device | cold load | first call | warm p50 | warm p90 | 1-min load avg |
|---|---|---|---|---|---|---|
| Laya, 421M | MPS | 103-340 s | 0.7-2.0 s | 51 ms | 67 ms | not recorded |
| Laya, 421M | MPS | | | 66 ms | 372 ms | 12.4 |
| Laya, 421M | MPS | | | 196-234 ms | 459-624 ms | about 15, during scoring |
| Laya, 421M | CPU | 135-163 s | 2.1-2.6 s | 759-1060 ms | 2.1-2.6 s | 6.4 |
| bge-small-en-v1.5, 33M | MPS | 11-56 s | 3.7 s | 55 ms | 155 ms | 22.3 |
| bge-small-en-v1.5, 33M | CPU | 101 s | 0.7 s | 80 ms | 173 ms | 20.2 |
| DeBERTa-v3-base NLI, 184M | MPS | 52 s | | 389 ms | 1.2 s | about 15, during scoring |

Laya's cold load is not reading the weights. In a profiled load, 185 of 207
seconds went to transformers randomly initialising ModernBERT before the
checkpoint overwrote it. A hook cannot load a model per prompt, so it needs a
resident server. Laya on CPU misses the 300 ms bar by a factor of three. On MPS
its p50 stayed under the bar in every run, and its p90 went over it under load.

### Classification

83 prompts: the 13 of `activation.eval.yaml` held out, plus 70 dev prompts, 59
paraphrased from real prompts and 11 synthetic. Five Laya framings and two
cheap baselines ran. The threshold for each was picked on the dev rows (the
highest recall at precision >= 0.90) and applied unchanged to the suite.

| method | AUROC dev | recall at P>=0.90, dev | suite negatives fired on | fires `s05` | clears the bar |
|---|---|---|---|---|---|
| Laya `noul`, task shape | 0.89 | 0.36 | none | yes | no, recall |
| Laya `noul`, team position | 0.72 | never reaches 0.90 | - | - | no |
| Laya `noul`, not an operation | 0.66 | 0.08 | none | yes | no, recall |
| Laya `choice`, two-way | 0.90 | 0.24 | none | no | no |
| Laya `choice`, four-way | 0.92 | 0.52 | none | yes | yes, by 0.02 |
| bge-small, anchor contrast | 0.90 | 0.72 | none | yes | yes |
| DeBERTa-v3 NLI | 0.81 | 0.36 | none | yes | no, recall |
| word list, no model | - | 0.32 | none | no | no |

`noul` is the calibrated primitive whose confidence was supposed to let the gate
abstain. Its best phrasing caught 9 of the 25 dev positives at the threshold,
and the other two were worse. The explicit `/kaibo:query` scored 0.09. No
framing fired on the explicit invocation.

Two framings clear the bar as written. Neither survives a closer look:

- **The thresholds are unstable.** Picked on a bootstrap resample of the dev
  rows and scored on the rows left out, a threshold held precision >= 0.90 in
  55% of 2000 draws for Laya four-way and 54% for the embedding.
- **Real traffic has fewer triggers.** The dev rows are 36% triggers; the
  sample of real prompts the set was drawn from was about 8%. Recomputed at 8%
  from smoothed rates, precision falls to 0.50 for Laya four-way and 0.48 for
  the embedding. About half of all nags would be false.
- **Seven framings, best of seven.** The bar was fixed in advance, but the
  number of framings was not.
- **The embedding's anchors were written by someone who had read the labels.**
  They paraphrase the description's rules, not the prompts, but the anchors
  were not written blind.

### Does the nag work, given a perfect gate?

On the suite, both passing gates fire on every positive and on no negative.
Four of the five positives already pass 3/3 on the description alone, so the
comparison comes down to one task: `s05`, *"Right, time to sort out how we
deploy this thing."* Caliper cannot install a hook, so a one-task spec appended
the nag line to the prompt instead. That is an upper bound on what a hook could
do. A release build of `4c3c406`,
`--model claude-code:claude-sonnet-5 --no-user-customizations`, k=3:

| arm | `query` fired | tokens per attempt |
|---|---|---|
| description only | 0/3 | 60K |
| with the nag | 3/3, each ran `kaibo doctrine deployment` | 128K |

Without the nag the agent saw an empty directory and asked which project was
meant, as in the
[2026-09-18 run](2026-09-18-activation-suite-run.md). With it, all three
attempts loaded the deployment doctrine first. So a well-placed nag fixes the
one prompt shape the description misses. The open question is how to place it
without also firing on the other half of the time.

## What was affected

Nothing in kaibo. No hook was built, and no skill description changed.

The cost of a Laya gate goes beyond its accuracy. It would need a resident
Python and torch server with 1.6 GB of weights, and several minutes to start
cold, next to a Rust binary whose `deps-stay-lean` gate refuses even an HTTP
client. [ADR 0009](../adr/0009-agents-consume-kaibo-on-task-entry.md) allows a
hook only as nag-only with no transport, and a localhost model server is a
transport in all but name. The upstream repository was created on 2026-09-18
and had 25,584 stars and 2,217 forks eight days later. Weights from a
repository that young, with growth like that, need pinning and review before
they run on every prompt.

## What changed as a result

Nothing. What a supervised gate would take, written down and not started:

1. **Labels at the real rate.** 1,500 to 2,000 first-person prompts from
   consenting transcripts, labelled by a strong model against the
   description's rules, with a human audit of every positive and of a random
   10% of negatives. The labels stay private; only paraphrases and aggregate
   numbers enter this repository.
2. **The cheapest head first.** Logistic regression or SetFit on bge-small
   embeddings, which ran about as fast as Laya here with under a tenth of the
   weights.
   Fine-tuning Laya itself comes second, and only if the small head falls
   short: it needs the upstream training scripts, and the base model's
   calibration has to be refit per bucket.
3. **Judge it on held-out real traffic.** Precision >= 0.90 at the real base
   rate, not on an enriched set, before any hook exists. Then run
   `activation.eval.yaml` with and without a real hook, which needs Caliper to
   be able to install one.
