---
status: accepted
---

# 0004. Cross-domain pages live with the constraint, not the technique

**Decided** 2026-07-07

## Context

Plenty of real pages sit at the intersection of two domains: a general
technique as it has to be applied under one platform's specific constraints.
Both domains are a plausible home, "both" means two copies that drift apart,
and a catch-all "recipes" domain is unowned by construction.

## Decision

An intersection page lives in the domain that supplies the **constraint or the
specificity**, not the one supplying the general technique. So a page about an
observability technique as constrained by a particular cloud platform lives in
the platform's domain.

It is tagged with the other domain's keywords and wikilinked from both sides.
One owner per page. No unowned "recipes" domain.

## Consequences

- Placement is decidable by rule rather than by discussion, and it lands the
  page next to the owner who holds the context that made it specific.
- Cross-domain discovery is carried by tags and wikilinks rather than by
  duplication, so there is one page to keep current.
