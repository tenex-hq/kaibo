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
