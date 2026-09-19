---
status: accepted
---

# 0016. One wide event per invocation, over OTLP

**Decided** 2026-09-19

Supersedes points 4 and 5 of
[0015](0015-the-paper-trail-measures-the-corpus-not-the-trigger.md), which
chose a bespoke local record with no transport. The central finding of 0015
stands untouched: the binary cannot measure the consumption trigger, because a
tool cannot record its own non-invocation.

## Context

0015 settled what the trail is for and then chose the smallest possible sink: a
purpose-built local file, no reader, no transport. Two things make that the
wrong call.

**It puts kaibo in violation of the doctrine kaibo serves.** The corpus page
`observability/reference/observability-principles.md` is prescriptive and
current. Principle 4 says logs, metrics and traces are implementation details
and to think in wide structured events. Principle 6 prefers a push-based OTLP
pipeline. Principle 10 says use OpenTelemetry. A bespoke local format is the
one shape the organisation has written down that it does not want.

**A narrow record re-commits to the mistake 0015 was written to avoid.** It
justified its field list by naming exactly one decision the trail serves.
Choosing fields against a single known question is how a trail ends up
recording the wrong thing precisely, which is the reason the observability
question was answered before the emission was designed at all.

The upstream standard also moved. OpenTelemetry deprecated the Span Events API
in March 2026 in favour of log-based events, and in the Rust implementation the
Logs API and SDK are Stable while traces remain Beta. One richly attributed log
record is both the idiomatic shape and the most stable surface available.

## Decision

**One wide event per `query` and `doctrine` invocation.** An OpenTelemetry log
record with `event.name`, carrying every dimension of the invocation as
attributes. No metrics, no spans.

**Attributes are derived from questions, never from what is easy to collect.**
The questions are recorded in the issue that builds this, each attribute maps
to at least one, and an attribute serving no question does not ship. Two
questions are recorded as permanently unanswerable and get no field that
implies otherwise: whether the answer was useful, and what fraction of domain
entries loaded doctrine.

**No spans, and the edge timings ride on the event.** kaibo makes no network
calls of its own; its edges are subprocess calls to `git` and `qmd`. A span
tree that is only ever a root and two children fits on the event as duration
attributes, which is the canonical-log-line discipline rather than a
concession. Spans stay additive if flat durations later prove too coarse.

**Two sinks, and they are not equals.** A local JSONL file is written
unconditionally, serialised by kaibo itself. OTLP export is additional, and
gated twice over.

- The file keeps the degradation ladder intact
  ([0002](0002-ai-optional-core.md)): the trail stays greppable on a machine
  with no collector. It is serialised with the crate kaibo already depends on,
  not by an exporter whose own documentation says the format is for debugging
  and subject to change.
- OTLP is behind a non-default cargo feature and a config key that defaults to
  off. The lightest OTLP-capable dependency set measures 115 packages against
  kaibo's 59, and pulls in the async and HTTP stack the binary has never had.
  Someone who wants `kaibo query` should not pay for a collector they do not
  run.

**The exporter never starts from ambient environment alone.** The standard
`OTEL_*` variables are honoured for conformance, but an endpoint found in the
environment does not by itself enable export: the kaibo config key must also
be set. `OTEL_EXPORTER_OTLP_ENDPOINT` is commonly exported machine-wide, the
event carries the questions someone asked, and inheriting a network target
ambiently is the failure `Config::resolve()` exists to prevent.

**Custom attributes are namespaced `kaibo.`** and never `otel.`, which the
specification reserves. The CLI semantic convention supplies
`process.executable.name`, `process.exit.code` and `process.pid`. The question
text is an attribute and deliberately not part of `process.command_args`,
which the convention itself flags as an argument-leakage path.

## Consequences

- #40 grows the event schema and the JSONL sink. OTLP export is separate work
  behind its own feature, so the dependency jump lands in its own reviewable
  change and can be declined without losing the trail.
- The trail gains a question 0015 could not have asked: whether a gap is a
  missing page or a page withheld as draft or unverified. `NoHits` currently
  fires on the filtered list, so these are indistinguishable today, and they
  call for opposite editorial actions.
- Latency becomes observable, which is the one trigger-adjacent fact the
  binary can see from the inside. It does not measure activation, but a tool
  slow enough to be avoided is a cause of low activation that the trail can
  now show.
- Export is a deliberate act on each machine, so the privacy position of 0015
  survives the addition of transport rather than being traded away for it.
- A reader verb is still not built. Grafana and `jq` are the readers, and
  0014's presumption against new verbs is undisturbed.

## Rejected

- **`opentelemetry-stdout` as the file sink.** Its documentation calls it a
  debugging aid whose format may change at any time, which is not a foundation
  for a durable on-disk trail.
- **A tracing SDK with spans per subprocess.** Beta in Rust, and it answers a
  latency question at a granularity nobody has yet needed.
- **Rewriting in a language with a lighter OTel story.** Go would genuinely
  cost fewer dependencies and has function-level auto-instrumentation that
  Rust lacks. Neither advantage reaches kaibo: no probe of any kind can
  observe a domain fact such as "this query found no results", so the
  instrumentation is hand-written in either language, and the dependency count
  is a build-time property that no user of the binary perceives.
  [0011](0011-rust-not-go.md) stands.
