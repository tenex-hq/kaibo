---
title: "Activation suite, k=3 on claude-sonnet-5"
---

# 2026-09-18: Activation suite, k=3 on claude-sonnet-5

A trigger-probe run of [`evals/activation.eval.yaml`](../../evals/activation.eval.yaml)
with the model pinned to `claude-code:claude-sonnet-5`. It predates two changes
that affect what it measured: the spec's skills were an older copy, not the ones
embedded in the binary, and the stub tools did not yet speak `kaibo query`'s
contract.

## What happened

36 of 39 attempts activated the expected skill (92.3%). Every task went 3/3
except one, which went 0/3. All four hardened negatives held, the
OpenTelemetry-SDK-syntax trap included, and all three lane-split probes landed
on the right skill.

The 0/3 task was *"Right, time to sort out how we deploy this thing."* Sonnet
fired nothing. It saw an empty working directory, judged the prompt too vague to
act on, and asked which project was meant without consulting the corpus. An
unpinned run on the default model over-fired on the same prompt (`query` plus
`sync`). The prompt failed in both directions on two models, which points at the
prompt shape, a bare domain entry with no project in front of it, rather than at
either model. The description's own worked example, *"lets do observability"*,
went 3/3.

## What was affected

The activation half of the query skill's `description`. Answer quality was not
measured.

## What changed as a result

Not recorded.
