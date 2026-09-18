# Parked

Things deliberately **not** built. Each carries a **trigger**: the observable
condition that would justify un-parking it. Absent the trigger, leave it alone.
A feature here is a good answer to a question the project has not earned yet.

## Deferred ideas

### 1. Contribution flywheel automation

The mechanism is already live: `query`'s proposal hook plus `contribute`'s
ratification note ([0003](0003-grounded-answers-and-labeled-proposals.md)).
Parked is everything beyond that - proposal queues, auto-drafted PRs,
ratification nudges.

*Trigger:* real query usage generating proposals worth ratifying, **and** the
manual hook proving too lossy.

### 2. Freshness automation

Who or what runs `sync`, and staleness detection beyond the last-commit-age
heuristic `query` already applies.

*Trigger:* stale answers becoming a real problem - now observable, via the
query staleness check.

### 3. A QMD MCP server instead of CLI subprocesses

Richer multi-search, at the cost of a server to run and manage.

*Trigger:* CLI ergonomics limiting what the skills can do.

### 4. An in-skill query log

One line per query - question, hit or miss, would-I-have-asked-a-human - to
make the success metric measurable and to feed the flywheel. Parked because a
log written by the skill puts a **write primitive into a read skill**, which is
exactly the capability narrowing that keeps the read path safe
([0007](0007-read-content-is-untrusted-data.md)).

A weaker substitute exists outside the skill: because every query shells out to
`qmd query "<question>"`, the questions are recoverable from agent telemetry
and can be replayed. What replay cannot give is a **verdict** - it yields a
similarity score, not whether the asker got what they needed. That verdict is
what remains parked.

*Trigger for the remainder:* the quantised qmd score proving too coarse to rank
content by.

See also [AXI](../axi.md), which sketches the way out: if the **binary** writes
the log, the agent never holds the write capability and the record is a true
hit/miss taken at the moment of retrieval.

### Killed outright, not parked

The consolidation in [0001](0001-single-knowledge-monorepo.md) killed rather
than parked: lazy per-repo materialization, fan-out and multi-select routing,
cross-cutting domain flags, clone pruning, a unified cross-repo index, and
routing-format-at-scale. They are answers to a problem the monorepo no longer
has.

Un-parked, in the other direction: atomic one-convention-per-page references
now live in the corpus conventions - their trigger, the consultant path turning
on, fired.

## Open questions

### Review gate for knowledge PRs

Correctness, staleness and contradiction detection. Who, or what, reviews a
knowledge PR? Merge is ratification today, which is a convention rather than a
check ([0008](0008-contribution-needs-read-access-not-write.md)). CODEOWNERS is
written per area and per domain so it can become enforcement without a
restructure.

### Lifecycle of outdated knowledge

How a page gets flagged `deprecated` or superseded. `updated:` records
authorship, not world-validity, and a confidently cited stale page is worse
than no page at all.

### Harvesting from an incumbent wiki

An existing wiki (Confluence or similar) holds content that kaibo answers will
want. External links are pointers, not knowledge, per the corpus conventions,
so linking out is not harvesting. There is no ingestion story yet.

### No-clone fallback

Answering without a local clone - forge code search over the monorepo - is
limited by private-repo visibility. Unresolved.
