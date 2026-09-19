---
status: accepted
---

# 0006. QMD isolated in a dedicated index with a single collection

**Decided** 2026-07-07

## Context

kaibo indexes the corpus with qmd, and qmd's default index is also where a
user's personal collections live. The first design kept the two apart with a
flag: scoping every indexing call to kaibo's own collection with `-c` was
assumed to bound what `qmd update` would touch.

On 2026-07-02 that guardrail was proven a no-op. The scoped update reindexed
the default database and pulled the user's personal collections in with it. The
guardrail had never been isolation, only an assumption about how a flag
behaved - and the failure was silent, because a successful reindex of the wrong
thing looks exactly like a successful reindex.

## Decision

kaibo uses a dedicated index, `--index kaibo`, with a single collection
`knowledge` and the mask `*/{reference,how-to,faq}/**/*.md` - the typed folders
inside domain folders, with root and navigation files excluded. Isolation is
structural: an operation addressed at the kaibo index cannot see, pull or
re-embed anything in the default one. Implemented 2026-07-07.

## Consequences

- The user's personal qmd usage and kaibo's are independent. Neither can
  corrupt the other, and neither has to know the other exists.
- A single collection means corpus-wide retrieval with no routing layer.
- The mask, not a filter at query time, is what keeps navigation files out of
  retrieval.
- The behaviour this rests on is qmd's, so it is pinned in the
  [QMD contract](../qmd-contract.md) and executable as an opt-in check
  (`KAIBO_QMD_CONTRACT=1 cargo test -p kaibo-core qmd_contract_check`), to be
  run when qmd upgrades. The original failure was an unverified behavioural
  assumption; the guard against repeating it is a test, not a sentence.
- Later hardened by [0010](0010-the-default-index-has-no-exceptions.md), which
  removed the last code path that named the default index at all.
