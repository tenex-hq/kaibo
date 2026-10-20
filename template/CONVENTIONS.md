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
| `faq/` | `faq` | none | Answered questions (Q&A) | …it's a short answer to a real question that doesn't justify its own reference/how-to page |

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

## What a page is for

A page states one position the organization takes, so that an agent at work
in another repo can find it, cite it, weigh it and act on it. That reader found
the page by meaning, cannot ask the author, cannot see the project the
knowledge came from, and treats a cited page as the team's answer. Every rule
below follows from that reader, and each one prevents a specific way the reader
fails.

These rules are judgement, so `kaibo lint` does not check them. Their decidable
parts (implied time, tracker references) belong in the corpus's prose lint; the
rest is what a reviewer, human or agent, reads a knowledge PR against. They
apply to new pages and to any page being edited. Existing pages are not swept.

### The admission test: it takes a side

A page holds a convention, a choice between options, a definition, a measured
trade-off, or the way a task is done here. The test is whether a competent
engineer elsewhere could have settled it differently. Pinning base images by
digest passes, because many teams pin by tag. A page that restates a tool's
documentation fails: it tells the reader nothing the vendor does not, lends the
restatement a team citation, and drifts from its source unnoticed. Public facts
appear only as the context a position needs.

The author does not need the organization to hold the position already. Review
is what makes the side the organization's, which is how a ratified labeled
proposal becomes a page.

### Citable: one position, stated first

A reference page holds one position, a how-to one task, an FAQ entry one
question ([why](#reference-pages-atomic)). The title names the claim ("Base
images are pinned by digest"), not the topic ("Docker images"); a definition, a
pattern or a method is titled by the name a reader searches for. Either way the
first sentence states the claim in full, so a reader who stops there, or a
search snippet that shows only that sentence, still carries it.

### Findable: in the words of the reader's situation

Search matches meaning, but the reader's question is concrete. A page about
where a check should run is found by "should this linter run pre-push?" only if
it names linters and pre-push. Name the trigger, the tools and the symptoms the
reader will have in hand, and keep tags specific.

### Argued: the reason, and what is known around it

- **The reason.** The mechanism or the measurement behind the claim. The reader
  has constraints the author never saw and can weigh the claim against them only
  by knowing what it protects. A claim with no reason is taste.
- **What lost.** Where the contributor knows it, the alternative a reader would
  reach for and why it lost. The obvious alternative is the one the next reader
  proposes, and recording why it lost ends that argument. A rejected
  alternative carries its verdict in its own sentence ("Rejected: ..."), because
  `query` returns a passage, not a page, and a passage that reads as advice is
  cited as advice.
- **Where it stops.** Where the contributor knows them, the conditions that
  change the answer, so a reader can tell whether their case is in scope.

**Record only what the contributor knows.** A reason, alternative or boundary
nobody stated is left out and named as an open question for the reviewer. One
inferred to fill the shape is a wrong page in waiting.

These are properties, not sections to fill in. A definition, a placement rule
and a runbook take different shapes, and the shape follows the claim. A binding
standard carries none of the argument itself: it holds the claim, its
exceptions and its checks, and cites the page that argues it
([what a standard page holds](#what-a-standard-page-holds)).

### Durable: nothing turns false unedited

A missing page is reported as a gap. A page that stopped being true is cited as
a grounded answer. A wrong page therefore costs more than a missing one, and
nothing on a page should expire silently.

**Evidence stays, live state goes.** Project detail on a page plays one of two
roles:

1. *Evidence for a durable claim*: a measured cost, a failure breakdown over a
   stated sample, a benchmark. It stays, with what was measured, where and
   when. A number earns its place when it is tied to the decision it produced:
   this cost led to that placement.
2. *A project's live state*: its current configuration, what runs where today,
   what is planned. It drifts, nothing in the corpus updates it, and the
   project's own repo is authoritative. It stays there.

Rejected: removing all project detail. The page becomes impersonal and never
stale, and also ungrounded: its claims turn into taste, and it stops matching
the concrete question that would find it. A worked example from one project
buys the reader three things:

- **Calibration.** The reader cannot reuse another repo's numbers but can reuse
  the derivation: measure the same quantities at home and compare them the same
  way.
- **The shape of the artifact.** One finished instance shows what a decision
  record or a placement table looks like, to be recognised rather than copied.
- **Retrieval surface.** Concrete terms match concrete questions.

**Anchor time, never imply it.** Tell a worked example in the dated past of the
decision, not the present of the project. "In March 2026 the team moved the
linter to pre-push, because it cost 8 seconds there against 100 in the build"
stays true. "The team runs the linter pre-push" goes false the day the hook
changes. "Currently", "recently", "we plan to", a ticket number and a person's
name fail the same way: each points at state the reader cannot see, which
changes without an edit to the page. Name the role or the owning team, and keep
plans in the tracker. A row that drifts was state, not evidence: cut it back to
the evidence rather than update it.

### What follows for the corpus

- **Length is paid by every task in the domain.** `kaibo doctrine` loads every
  `current` reference page of a domain in full. Evidence earns that cost;
  narrative does not.
- **Contradictions are resolved, not left standing.** A page that contradicts
  another, binding or not, improves or supersedes it in the same PR. Two
  current pages that disagree turn a grounded answer into a coin toss.

### Review questions

1. Could a competent engineer elsewhere have settled this differently?
2. Is there one position, stated in full in the first sentence?
3. Would a concrete question from another repo, in that repo's words, reach it?
4. Does it give the reason and, where known, what lost and where it stops, with
   nothing invented?
5. Is every number tied to the decision it produced, with a source and a date?
6. Would every sentence still be true in a year if nothing changed but the
   calendar?
7. Does another page already hold this position, or its opposite?

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

### One claim per standard

[Atomicity](#reference-pages-atomic) is load-bearing here rather than merely
tidy. A contract returns **one verdict per standard**, so a page binding three
claims produces a verdict nobody can read: which of the three failed? Keep one
claim per binding page, and split it the moment a second one appears.
`kaibo lint` does not check this today - counting claims in prose is a
judgment call, and a rule that got it wrong would block a contributor over a
guess - so this stays a discipline the page's author holds, not something the
registry enforces.

### What a standard page holds

The claim, its exceptions, and its checks. The reasoning stays on the page
the claim came from, which cites the standard and is cited back. One
argument, one place: a standard that restates the argument beside the rule
creates a second copy, and the second copy is the one nobody edits when the
first one changes.

A compiled contract carries the lead claim, the severity, the checks and the
citation, not the page. That is what keeps page length free. Write as much
context behind the citation as a human reader needs, and the caller about to
act still receives one sentence and a verdict.

A standard also does not describe what its own checks cannot decide. Writing
"this pattern cannot see X" does not make it see X, and it puts a weaker
second statement of the rule beside the real one. State the rule in full and
let the checks be a partial witness to it. Which checks a contract issued and
which came back with a verdict is the contract's report, not the page's
prose.

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
