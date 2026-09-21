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

### 4. The query log's verdict half

One line per query - question, hit or miss, would-I-have-asked-a-human - to
make the success metric measurable and to feed the flywheel. Parked because a
log written by the skill puts a **write primitive into a read skill**, which is
exactly the capability narrowing that keeps the read path safe
([0007](../adr/0007-read-content-is-untrusted-data.md)).

The way out sketched in [AXI](../axi.md) - if the **binary** writes the log,
the agent never holds the write capability - has shipped: the binary now
writes an append-only local trail of `query` and `doctrine` invocations, with
OTLP export as an opt-in second sink
([0015](../adr/0015-the-paper-trail-measures-the-corpus-not-the-trigger.md),
[0016](../adr/0016-one-wide-event-per-invocation-over-otlp.md)).

What remains parked is the **verdict** in its original sense: a felt
would-I-have-asked-a-human, which a tool cannot supply because it cannot
record its own non-invocation. What un-parked instead is a narrower verdict
the trail can actually give: **gap recurrence**, which knowledge gap keeps
showing up across the invocations that did happen. The original question,
activation, is answered separately by the eval suite, not by the trail
([0015](../adr/0015-the-paper-trail-measures-the-corpus-not-the-trigger.md)).

*Trigger for the remainder:* none - 0015 argues no record shape closes the
non-invocation gap, so the felt-sense verdict is not waiting on a future
trigger; activation is already measured by the eval suite instead.

### Killed outright, not parked

The consolidation in [0001](0001-single-knowledge-monorepo.md) killed rather
than parked: lazy per-repo materialization, fan-out and multi-select routing,
cross-cutting domain flags, clone pruning, a unified cross-repo index, and
routing-format-at-scale. They are answers to a problem the monorepo no longer
has.

Un-parked, in the other direction: atomic one-convention-per-page references
now live in the corpus conventions - their trigger, the consultant path turning
on, fired.
