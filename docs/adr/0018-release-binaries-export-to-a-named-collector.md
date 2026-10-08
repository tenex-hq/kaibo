---
status: accepted
---

# 0018. Release binaries export, to a collector kaibo's config names

**Decided** 2026-10-08

Amends [0016](0016-one-wide-event-per-invocation-over-otlp.md): its two
export gates, a non-default cargo feature and a boolean config key, become one
gate, a configured collector URL. The wide event, both sinks and the rule that
ambient `OTEL_*` never starts export all stand.

## Context

0016 put OTLP export behind a cargo feature that is off by default, and
behind an `otlp_export` switch that left the target to the SDK's ambient
`OTEL_EXPORTER_OTLP_ENDPOINT`. Each gate is defensible alone. Together they
make the pipeline 0016 designed unreachable in practice.

**No installed binary can export.** The release workflow builds with default
features, so every binary from the Homebrew tap or the shell installer lacks
the exporter. Switching export on there changes nothing, and nothing says so.
The feature only ever served someone building from source.

**The cost the feature guarded against is not one a user perceives.** 0016's
own Rejected section says so while declining Go: "the dependency count is a
build-time property that no user of the binary perceives." The OTLP stack
roughly doubles the package count of a build, which matters to whoever
compiles kaibo and to nobody who runs it.

**The ambient endpoint is the wrong place to name the sink.** kaibo is mostly
run by an agent harness, and a harness's shell children do not reliably
inherit `OTEL_*` variables; when they do, the values describe where the
harness reports, not where kaibo should. The SDK's fallback,
`localhost:4318`, is not where a given collector necessarily listens. A
boolean in kaibo's config plus a URL in the environment gave the user two
places to get right, and no single one to read back.

## Decision

**Release builds carry the exporter.** `dist-workspace.toml` sets
`features = ["otlp"]`, so every binary cargo-dist builds includes it. The
default `cargo build` stays lean, and `deps-stay-lean` keeps guarding exactly
that.

**One kaibo-owned key is both switch and target.** `otlp_endpoint` in
`~/.kaibo/config.toml`, or `KAIBO_OTLP_ENDPOINT`, is the collector's OTLP/HTTP
base URL. The event is sent as http/protobuf to `<base>/v1/logs`, the path the
specification appends to a base endpoint. Set, an exporter is built and pointed
there explicitly; unset or empty, no exporter is built at all.

**kaibo's key wins over every ambient one.** The URL is handed to the exporter
programmatically, which in the SDK outranks `OTEL_EXPORTER_OTLP_ENDPOINT` and
`OTEL_EXPORTER_OTLP_LOGS_ENDPOINT`. An ambient endpoint alone still never
starts export, as 0016 requires. The export timeout keeps kaibo's two-second
default and `OTEL_EXPORTER_OTLP_TIMEOUT`; headers still come from the standard
`OTEL_EXPORTER_OTLP_HEADERS`.

**The retired switch is refused when it says "on", not ignored.**
`otlp_export` in the config file set to anything but `false`, or a
`KAIBO_OTLP_EXPORT` that is not an explicit no (`0`, `false`, `no`, `off`),
stops resolution with an error naming the replacement. Ignoring it would leave
someone believing they export while nothing leaves the machine, which is the
failure this record exists to end. An explicit "off" cannot cause that belief,
so it is accepted as a no-op and `kaibo status` lists it as a key to remove.

**An endpoint the exporter cannot use builds no exporter.** `otlp_endpoint`
must be a plain `http://` URL with a host and, optionally, a numeric port. Any
other scheme, `https://` included, or a malformed value leaves export off
rather than building an exporter that only looks like it works. The verbs are
unaffected and `kaibo status` names the reason and the fix.

**`kaibo status` reports the target and its source**, and names a configured
endpoint on a binary built without the feature as a finding with its fix.

## Consequences

- The privacy position of 0015 and 0016 survives intact. Compiling the
  exporter in sends nothing; an event leaves the machine only when someone has
  written a collector URL into kaibo's own config. Export stays a deliberate
  act on each machine.
- Release binaries are larger and their dependency closure roughly doubles.
  Nobody running them can tell, which is the point 0016 already made.
- Someone building from source still opts in with `--features otlp`, and
  `status` tells them when they configured a collector for a build that
  cannot reach it.
- A local collector can now see every invocation from an installed binary,
  which is the first time the pipeline 0016 designed is reachable at all.
- The exporter carries no TLS stack, so `otlp_endpoint` is a plain `http://`
  URL. A remote collector is reached through a local one or a TLS-terminating
  proxy; adding TLS to kaibo is a separate dependency decision, not taken here.

## Rejected

- **Making `otlp` a default feature.** It would put the async and HTTP stack
  into every `cargo build` and every CI job, for a property only the shipped
  artifact needs. Enabling it at the release boundary costs nothing elsewhere.
- **Keeping the boolean and reading the target from `OTEL_*`.** It keeps two
  places to configure one thing, and the ambient one is exactly what 0016
  decided must not steer where the event goes.
- **Silently ignoring `otlp_export`.** Quietly turning export off for anyone
  who had it on is worse than one error that says what to write instead.
- **Refusing `otlp_export = false` too.** It stops every verb over a line
  that already says what kaibo now does by default. A status finding is
  enough.
- **Failing resolution on an unusable `otlp_endpoint`.** The endpoint only
  feeds the second sink, and an unreachable collector already must not change
  a verb's result. A bad value is the same kind of fault, reported the same
  quiet way.
