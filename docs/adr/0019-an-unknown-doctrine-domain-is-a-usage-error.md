---
status: accepted
---

# 0019. An unknown doctrine domain is a usage error, not a gap

**Decided** 2026-10-08

## Context

`kaibo doctrine <domain>` reported an argument matching no root-MOC heading as
a knowledge gap, exit 3. The query skill's trigger lists kinds of work
("observability, deployment, testing, agentic patterns, docs, or tooling"),
and agents passed those words straight through as domain names. The trail
shows `doctrine deployment` and `doctrine testing` exiting 3 against a corpus
whose `engineering-practices` domain covers testing in depth. The agent was
told the organisation knows nothing, which was false.

That also broke the exit-code contract in `AGENTS.md`: a caller reading only
the exit code must be able to tell a knowledge gap from a failure. A name that
selects nothing never asked the corpus anything, so it cannot be a gap.

## Decision

**Domain names match exactly, as before.** An argument matching no heading
exits 2 (usage), listing the real domain names and naming `kaibo domains` as
the fix. Exit 3 from `doctrine` now means one thing: a real domain with
nothing current to load.

**The skill stops implying its trigger words are domain names.** It keeps the
kinds-of-work wording, which is what makes the trigger followable
([0009](0009-agents-consume-kaibo-on-task-entry.md)), and tells the agent to
pass the domain the work falls in, checking `kaibo domains` for the names.

**The trail tells a wrong name from a gap.** `kaibo.subject` keeps the
argument; `kaibo.domain` is absent when the argument named no domain, and the
outcome is an error with exit 2. No attribute is added.

## Consequences

- An agent that guesses a name gets the real names in the same response and
  can re-run, instead of reporting a gap that does not exist.
- A caller that branched on exit 3 for an unknown domain now sees 2.

## Rejected

- **Resolving a non-name by matching it against each domain's topics,** and
  normalised or fuzzy name matching with it. It was built and withdrawn. It
  guesses: a word picks a domain because it appears in a topic list, and one
  topic list overlapping another silently changes which doctrine loads. It
  makes every topic list something to maintain for routing as well as for
  description. And it treats the symptom: the cause was the skill's wording,
  which is now fixed at the source.
