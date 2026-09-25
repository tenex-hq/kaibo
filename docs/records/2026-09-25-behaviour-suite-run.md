---
title: "Behaviour suite, full and query-ablated arms, k=3 on claude-sonnet-5"
---

# 2026-09-25: Behaviour suite, full and query-ablated arms

The first behaviour run after the stub tools were made to speak `kaibo query`'s
contract, and so the first whose numbers describe the suite in
[`evals/behaviour.eval.yaml`](../../evals/behaviour.eval.yaml). Every earlier
behaviour number was measured while the stubs were broken.

## What happened

Both arms ran from commit `1075f80`, against a release build current with its
sources, via [`evals/README.md`](../../evals/README.md):

```bash
uvx --from caliper-eval caliper run evals/behaviour.eval.yaml --k 3 --timeout 300 \
  --model claude-code:claude-sonnet-5 \
  --judge-model claude-code:claude-haiku-4-5-20251001 \
  --no-user-customizations            # plus --ablate query for the second arm
```

No attempt was UNUSABLE in either arm.

| Arm | Passed | Score | Attempt tokens |
|---|---|---|---|
| full | 18/21 | 85.7% | 2.7M |
| `--ablate query` | 7/21 | 33.3% | 4.1M |

### Full arm

Six tasks passed 3/3. *Grounded answer cites repo-relative paths* failed 0/3.

All three attempts ran `kaibo query`, cited
`event-schemas/reference/schema-registry.md` by the right path, and recommended
Quillrail. All three gave one of the two reasons the rubric asks for, that the
compatibility check runs at publish time, and none gave the other, that
Quillrail reads the same contract file as the CI linter. The judge failed each
on the missing reason.

The missing reason is in the page body and not in the snippet, and the query
skill cannot get past the snippet. Its `allowed-tools` are `kaibo query`,
`kaibo doctrine` and `kaibo domains`, and `kaibo query` renders one fenced
snippet per hit. The stub's snippet is one sentence, shorter than the few lines
of context qmd's real snippets carry, so the stub made the loss larger than a
real corpus would. The loss itself does not depend on the stub: any rationale
longer than the excerpt gets cut off.

### Ablated arm

The 33.3% does not measure the bare agent. 7 of the 21 attempts reached kaibo
knowledge without the skill, and 4 of the 7 passes were among them:

- 4 attempts searched the host disk, found the operator's real
  `~/.kaibo/knowledge`, and read pages from it.
- 4 attempts called `kaibo` directly, which the `stubs/kaibo` shim serves in
  both arms. One got there through the `sync` skill.
- One attempt did both.
- One pass on *Answer is the delta* came from a judge that timed out at 60s.
  Caliper counted it as a pass rather than dropping it.

Of the 14 attempts that reached nothing, 3 passed. Two of those were *Reports a
gap*, which the bare agent passes by saying it has nothing, so that task does
not tell the arms apart.

## What was affected

Only the eval harness and the question of whether the skill is worth its
context cost. No code path in kaibo changed. The full arm made no attempt to read
the host disk.

## What changed as a result

- `evals/README.md` records the host-disk, CLI and judge-timeout behaviour as
  harness traps.
- `.caliper/` is gitignored. Caliper writes each run's transcripts there, and
  this run's quote the operator's real corpus.
