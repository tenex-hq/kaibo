# Kaibo Conventions

The single source of truth `kaibo lint` checks the corpus against, and that
`query`, `contribute` and `sync` all read. A new domain works with **zero code
changes** as long as it follows these.

## The corpus

All knowledge lives in **one repo** - the knowledge repo configured for your
kaibo install (see `Config::repo()`) - one folder per domain, each with typed
subfolders:

```
<knowledge-repo>/
├── _index.md        ← root MOC: one section per domain
├── CODEOWNERS       ← one line per domain folder
├── README.md        ← minimal navigation (not indexed)
├── kaibo/
│   ├── reference/  how-to/  faq/
├── observability/
│   ├── reference/  how-to/  faq/
└── …
```

## Content types

Content is organized by **purpose**, not one-file-per-question. The lens is
[Diátaxis](https://diataxis.fr), mapped onto kaibo's vocabulary. Three types:

| Folder | `type:` | Diátaxis | Holds | A page is a good fit when… |
|--------|---------|----------|-------|-----------------------------|
| `reference/` | `reference` | Reference + Explanation | Guidelines, commandments, baselines, standards | …it states how something *is* or *should be*, stably, independent of any one task |
| `how-to/` | `how-to` | How-to | How-do-I answers, runbooks | …it walks through *doing* a specific task, start to finish |
| `faq/` | `faq` | — | Answered questions (Q&A) | …it's a short answer to a real question that doesn't justify its own reference/how-to page |

> `tutorial/` (learning-oriented) is intentionally deferred. Add it only when a real domain needs it (and extend the indexing mask below).

## Frontmatter contract

Every knowledge page starts with YAML frontmatter. Required fields:

```yaml
---
type: reference | how-to | faq   # must match the folder; drives `contribute` placement
title: Human Readable Title       # used in citations and search titles
tags: [kebab-case, keywords]      # kebab-case only: lowercase, hyphen-separated
status: current | draft | deprecated
updated: YYYY-MM-DD               # last meaningful edit
---
```

- `contribute apply` sets `status: draft` on brand-new pages until a human
  reviews the PR. `query` excludes drafts from grounded answers by default (or
  surfaces them explicitly labeled with `--include-drafts`).
- `deprecated` pages stay in the repo (history matters) but are demoted in
  answers.

## Wikilinks

Link related pages with `[[page-filename-without-extension]]`
(Obsidian-flavored), e.g. `[[error-handling-guidelines]]`. Links resolve
**repo-wide** - the corpus is one vault namespace - so the convention that
makes them unambiguous is: **keep page basenames unique across the whole
repo.**

- **Cross-domain links go both ways.** GitHub renders no backlinks, so when a
  page links into another domain, add a "see also" link back from that
  domain's most-related page. (Obsidian users get backlinks and the graph for
  free - the local clone opens directly as a vault.)
- **External links (wikis, blogs, docs sites) are pointers, not knowledge.**
  Fine as "further reading" - but if an *answer* would depend on the external
  content, harvest it into a page instead. A citation to a page you don't
  control is a citation you can't stand behind.

## Cross-domain pages

An intersection page (e.g. `otel-in-azure-functions.md`) lives in the domain
that supplies the **constraint/specificity** (Azure), not the general
technique (observability). Tag it with the other domain's keywords, wikilink
it from both sides, one owner (the folder's CODEOWNERS line). No unowned
"recipes" domain.

## Writing style

Two structural rules are worth enforcing in CI, whatever your lint tool of
choice:

| Rule | Means |
|------|-------|
| no em dashes or en dashes as prose punctuation | use a comma, colon, period, or parentheses |
| no `--` standing in for a dash | backtick it when it is a CLI flag |

## Reference pages: atomic

One convention per `reference/` page - sharp primitives compose, sprawling
pages don't. This matters doubly now that `query` may combine primitives into
**labeled proposals**: a page that states exactly one thing can be cited,
composed, and contradicted cleanly. If a reference page grows an unrelated
second topic, split it.

## Binding standards

Normativity is an axis, not a content type. Diátaxis splits on documentation
*purpose*, and "this is binding" is not on that grid, so a standard stays
`type: reference` and grows four optional keys. **Every existing page is
non-binding by omission** - nothing needs migrating, and a corpus that never
uses these keys never notices they exist.

```yaml
---
type: reference
title: Library code logs, it does not print
tags: [python, logging]
status: current
updated: 2026-09-21
binding: true            # a boolean, and the key everything else hangs off
severity: must           # must | should
applies_to:
  actions: [file-edit]   # the closed vocabulary below, at least one
  tags: [workload-repo]  # free, optional: these narrow, they do not address
---
```

`severity` is the budget knob, and has two values: `must` is always in scope,
`should` is included on request. It is not a scale to argue about.

`applies_to` keys on **the action the caller is about to take**, which the
caller always knows. Not a filename glob: a glob is a proxy for the action and
a poor one, firing a commit-message rule on every README and unable to say "a
compose file *in a workload repo*". The action vocabulary is closed and
extended deliberately, never per standard:

| action | the caller is about to… |
|--------|--------------------------|
| `file-edit` | write or change a file |
| `commit-message` | write a commit message |
| `shell-command` | run a command |
| `chat` | answer in prose |
| `deploy` | ship something |
| `adr` | record a decision |

**All or nothing.** A page carrying any one of these keys carries all of them.
Half a standard - a `severity` nobody binds, a checks block on a page that is
not `binding: true` - is a page that looks binding and is not, so `kaibo lint`
refuses it rather than loading the half that parsed.

### Checks

A binding page may carry one `kaibo-checks` block in its body: the checks a
client runs locally, against the artifact, before it acts. They produce
**facts** - which rule, where, what matched - and need no model at all.

```json kaibo-checks
[
  { "id": "no-print", "kind": "forbid_regex", "pattern": "^\\s*print\\(" },
  { "id": "has-spdx", "kind": "require_regex", "pattern": "SPDX-License-Identifier" },
  { "id": "test-asserts", "kind": "require_if_present", "if_present": "def test_", "require": "assert " },
  { "id": "not-vendored", "kind": "forbid_path", "pattern": "^vendor/" }
]
```

| kind | fields | holds when |
|------|--------|------------|
| `forbid_regex` | `pattern` | the artifact's text does **not** match |
| `require_regex` | `pattern` | the artifact's text matches at least once |
| `require_if_present` | `if_present`, `require` | wherever `if_present` matches, `require` matches too |
| `forbid_path` | `pattern` | the artifact's **path** does not match |

- **Checks are JSON in the body, not YAML in the frontmatter.** A regex inside
  YAML is a quoting minefield - `\s*`, `\\s*` and a double-quoted scalar all
  mean different things - and this corpus has to stay hand-editable. JSON
  escaping is unambiguous.
- The fence is marked `json kaibo-checks`: `json` so GitHub highlights it,
  `kaibo-checks` because that token is what kaibo looks for. **One block per
  page.** A block quoted inside a wider fence is an example, not a block, so a
  page may document the schema without becoming a broken standard.
- Every `pattern` is a [Rust `regex`](https://docs.rs/regex) expression, and
  `kaibo lint` compiles it. A pattern that does not compile fails in the
  corpus's CI rather than on the machine of whoever took the action.
- `id` is what a verdict is keyed on: non-empty, unique within the page.
- **Checks are optional.** A standard no regex can express is still binding,
  still carries a severity, and still reaches the caller as prose. Requiring
  checks would quietly exclude most real standards.

### No judgment checks

The schema has one check species, the decidable one. A second was specified -
a rubric item a model answers rather than a check anything runs - and
[`evals/h2`](../evals/h2/README.md) measured it against the same standards
written as prose, on two model tiers: it bought no recall, at 2.6x to 3.3x the
cost. A page writing `kind: judgment` gets a schema error, not a silent
drop. See [ADR 0017](../docs/adr/0017-the-conformance-schema-is-decidable-only.md).

## `_index.md` - the root MOC

One section per domain folder, in this fixed format:

```markdown
## <domain>
- **owner:** @github-handle
- **domain:** <one line>
- **topics:** comma, separated, keywords
- **summary:** 1-2 sentences describing what ACTUALLY lives here (inventory, not aspiration)
```

`query` reads it for gap-reporting ("no page covers X; nearest domain: Y");
`contribute` reads it to pick the domain folder. It is **not** consulted at
retrieval time - semantic search covers the whole repo.

## FAQ page shape

One question per `##` heading, answer immediately below, link out to the
fuller page when one exists. Keep answers to a few sentences - an FAQ that
grows a section into an essay should graduate that section to a `reference/`
or `how-to/` page.

## Indexing (what QMD sees)

One collection, in a dedicated index:

- **Index:** `kaibo` (`--index kaibo` on every kaibo qmd call) - a separate
  database, structurally isolated from the user's personal collections.
- **Collection:** `knowledge` → the local clone of the configured knowledge
  repo.
- **Mask:** `*/{reference,how-to,faq}/**/*.md` - only pages inside typed
  folders inside domain folders are retrievable. Root files (`README.md`,
  `_index.md`, `CODEOWNERS`) and stray domain-level files are **navigation,
  not knowledge**, and stay out of search. Extend the mask if a new
  content-type folder (e.g. `tutorial/`) is added.

**Making things queryable is `kaibo sync`'s job** - it clones/pulls, applies
the mask, reindexes, and embeds. Never run ad-hoc `qmd collection
add`/`qmd update` for kaibo content by hand.

Keep the repo's `README.md` **minimal** - a short landing page (what the repo
is, its layout). It's the GitHub front door, not a knowledge page.

## Adding a domain

A domain is a folder, not a repo. Copy the domain template into the corpus as
`<domain>/`, add its `## <domain>` section to `_index.md`, add its
`CODEOWNERS` line, and open a PR - all in one change. `contribute apply` does
this automatically when it files knowledge into a domain that doesn't exist
yet. After merge: `kaibo sync`.
