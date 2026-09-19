---
status: accepted
---

# 0013. The skills ship embedded in the binary

**Decided** 2026-09-17

## Context

Splitting orchestration into a binary while leaving the skill prose as markdown
in a repository has one genuine cost: the two version apart, and then drift.
Prose says *when* to invoke, the binary says *how*; when they can be at
different versions, a prompt can instruct a mechanism that does not exist on
the machine reading it.

This has already bitten the project once. A security narrowing of a skill's
`allowed-tools` shipped to the repository and never reached the machine that
was running the skill.

## Decision

The skills are embedded in the binary, and `kaibo install` writes them out as a
skills-directory plugin. One artefact, one version, one `brew upgrade`
([0012](0012-distribute-via-a-generated-homebrew-tap.md)) updating prose and
mechanism atomically.

The namespace survives without a marketplace. Claude Code adopts any non-hidden
directory under `~/.claude/skills/` (and `<project>/.claude/skills/`) as a
plugin named after the directory, with id `<name>@skills-dir`. Writing the full
plugin shape - `~/.claude/skills/kaibo/.claude-plugin/plugin.json` plus
`skills/{query,contribute,sync}/SKILL.md` - yields `/kaibo:query` with no
marketplace registration and no `settings.json` mutation. A flat `SKILL.md` at
that directory's root would instead give the unnamespaced `/kaibo`. The
marketplace manifest and the plugin install instructions are deleted.

## Consequences

- Skill prose and the mechanism it describes cannot be at different versions on
  a machine, which is the drift this decision exists to remove.
- **Adoption happens at session start, not live.** A freshly installed or
  upgraded skill is not picked up by a running session, so `kaibo install` has
  to say so rather than leaving the user to discover it.
- **Managed enterprise settings can block the `skills-dir` source wholesale.**
  Where they do, `kaibo install` does not make the skills available. This is a
  convenience path, not a bypass, and must not be documented as one.
