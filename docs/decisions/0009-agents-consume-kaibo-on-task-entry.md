# 0009. Agents consume kaibo on task entry, not on felt gaps

**Status** Accepted
**Decided** 2026-08-29

## Context

The earlier rule was gap-based: query the corpus when you lack team knowledge.
It does not work, and the reason is established by experiment - do not
re-litigate it.

An agent was asked how to deploy developer tooling for developers. It answered
from the open repository alone and never queried. Challenged afterwards, it
correctly recited the rule it had just skipped. The instruction was in context
and was understood, so **post-hoc articulation is not evidence of pre-hoc
triggering**.

The root cause is that there was no *felt* gap. Local evidence in an open repo
beats a remote lookup every time, so an agent with a repository in front of it
never experiences the state the rule is conditioned on. A gap-based rule is
unfollowable for exactly that reason. A task-entry rule is followable, because
an agent always knows what it is about to do.

## Decision

Consumption triggers on what the task *is*, in two clauses.

1. **Doctrine load.** On entering a domain of work, query that domain before
   touching anything. The trigger is the task - "lets do observability" - once
   per domain per session. Mechanical, high compliance.
2. **Research.** Before proposing, planning or deciding anything non-trivial,
   ask what has been tried, what was rejected and why, and whether the org has
   a position at all. Iterative, judgement-led, lower compliance and higher
   value.

Clause 1 exists partly to make clause 2 likelier: an agent that has consulted
kaibo once in a session goes back.

Having a repository open that appears to answer the question is not a reason to
skip either clause. That is the failure mode, not an exemption from it.

## Consequences

- The skill description and the standing agent instructions are written around
  task shape ("the first turn that commits you to this domain"), not around
  uncertainty.
- Compliance is now observable: either the query happened on entering the
  domain or it did not, which a transcript shows without interpretation.

## Rejected

- **Strengthening the prose again.** That experiment has already been run - the
  standing agent instructions were strengthened and the skill description
  retuned, and the failing transcript above is the result of that version.
- **A prompt-submit hook that retrieves over HTTP and injects citations.** It
  would create a third read surface with its own auth and failure modes,
  alongside the skills' local clone and the hosted MCP tenant.

## Revisit when

The task-entry rewrite has been measured, and not before. If a hook is ever
built it must be nag-only: match the prompt shape, print one line of additional
context, exit 0, no transport.
