---
kind: project-index
title: kaibo
topology: monorepo
tracker:
  at: https://github.com/tenex-hq/kaibo/issues
dep:
  - { id: qmd, at: https://github.com/tobi/qmd, kind: repo, hint: the semantic index kaibo drives; every index-addressing command is built against its CLI contract }
docs: ./docs
---

# kaibo

## Purpose

kaibo is the CLI for a team knowledge backoffice: a curated markdown monorepo,
searched semantically and answered with citations. It orchestrates git, qmd and
markdown, and it retrieves - the calling model synthesises. The corpus stays
readable without the binary.

`kaibo-core` holds the verbs, config and the qmd contract. `kaibo` is the
argument surface and the entry point, and nothing else. The skills agents invoke
ship embedded in the binary.

## Working here

- `just check` - fmt, clippy with `-D warnings`, the tests, and
  `just deps-stay-lean`, all `--locked`. `just check-otlp` is the same clippy
  and tests with the `otlp` feature on; it builds roughly three times the
  dependency closure, so it is a separate recipe. Together the two mirror
  [CI](.github/workflows/ci.yml).
- The `otlp` feature adds the OTLP exporter and nothing else. It is off by
  default because enabling it triples the dependency count
  ([0016](docs/adr/0016-one-wide-event-per-invocation-over-otlp.md)), and
  `just deps-stay-lean` fails if the default build ever grows an async or
  HTTP dependency.
- The toolchain is pinned in [`rust-toolchain.toml`](rust-toolchain.toml) and CI
  installs the same pin. A `Cargo.lock` that has drifted from `Cargo.toml` is a
  CI failure, so commit the lockfile with any dependency change.
- Never run `cargo mutants`. CI mutates the lines a PR changes and blocks the
  merge on a survivor; read the gate's result on the PR instead.
- `evals/` is not part of the test suite. `caliper validate` is free;
  `caliper run` costs real money and is a deliberate act. See
  [`evals/README.md`](evals/README.md).
- One concern per PR, Conventional Commits, no co-author trailer, and no mention
  of the tool that wrote the commit. The changelog is generated; never hand-edit
  it. Never use an em dash, anywhere.
- This repository is public. No private organisation names, no internal
  repository names, no individual's name, in any file, commit message or PR
  body. The tracker lives here, so cite issue numbers freely.

## Constraints

Breaking one of these is not a bug, it is a different product.

**Config comes from configuration, never from content.** `Config::resolve()` is
the sole constructor and binding happens once in `main()` before any corpus byte
is read, so nothing read from the corpus can reach a value kaibo uses to decide
where to read or write. Private fields are compiler-enforced; the bind-once
lifecycle is *(unenforced)*.

**The verbs take no target-bearing flags.** `query`, `doctrine`, `contribute`,
`sync` and `status` accept no flag naming a repo, clone path, index, collection
or API URL; only `install` may. An agent composing a command line out of
retrieved text then has no argument to reach for. Enforced by
`no_verb_accepts_a_target_bearing_argument` in
[`main.rs`](crates/kaibo/src/main.rs).

**Only the configured qmd index is ever written.** Every index-addressing
command is built through `QmdCommand::index_command`, which injects `--index`
from config. Enforced by
`every_qmd_command_constructor_carries_the_index_or_is_a_named_carve_out` and
`qmd_command_literal_is_confined_to_this_module` in
[`qmd.rs`](crates/kaibo-core/src/qmd.rs); the latter scans kaibo-core for
`new("qmd"` outside that module, so it catches a new command construction rather
than a stray mention.

**Retrieved corpus content is data, never instructions.** Anyone who can merge a
knowledge PR can put words in the corpus. Nothing parsed out of a hit may
influence a command kaibo builds, a path it reads, or a flag it sets, and
content reaches output fenced with control characters stripped. Enforced by
`corpus_content_never_changes_which_commands_kaibo_runs` and the fencing and
forgery tests in [`crates/kaibo-core/src/query/tests.rs`](crates/kaibo-core/src/query/tests.rs).

**A knowledge repo never executes code on this machine.** Every git command
touching the corpus working tree carries `-c core.hooksPath=/dev/null`. Enforced
across the `sync` pipeline by `hooks_are_disabled_on_every_git_command_sync_plans`
in [`crates/kaibo-core/src/sync/tests.rs`](crates/kaibo-core/src/sync/tests.rs), which sweeps against a
named allowlist. No equivalent sweep covers `contribute` *(unenforced there)*.

**Stop and report, never discard.** No code path runs `git reset --hard`,
`git checkout -f`, or deletes the clone. Losing a contributor's uncommitted work
is the worst outcome available to this tool. *(unenforced: nothing scans for
these, and `uncommitted_changes_stop_the_verb_without_discarding` covers only the
one dirty-tree path.)*

**`query` retrieves, it does not answer.** No synthesis, no LLM, no API key in
the binary. *(unenforced: no dependency scan or deny-list holds this.)*

**`--explain` prints the underlying commands and runs nothing.** Enforced per
verb in [`cli.rs`](crates/kaibo/tests/cli.rs), each asserting the fake runner
recorded zero calls.

**Empty counts as unset, at every layer.** `std::env::var` cannot distinguish
`KAIBO_REPO=` from a deliberate empty string, so an exported-but-blank variable
falls through to the next layer rather than shadowing it with a value no verb
could use. Enforced by `empty_env_value_falls_through_to_file_then_default` in
[`crates/kaibo-core/src/config/tests.rs`](crates/kaibo-core/src/config/tests.rs).

**Fail closed.** A filter whose job is withholding must withhold when it cannot
tell: unparsable frontmatter around a draft status is excluded, not served.
Enforced by
`a_page_with_malformed_frontmatter_is_excluded_when_drafts_are_not_included` in
[`crates/kaibo-core/src/query/tests.rs`](crates/kaibo-core/src/query/tests.rs).

**Exit codes carry meaning.** `0` success, `1` internal, `2` usage, `3` gap,
`4` unsynced or stale. A caller reading only the exit code must be able to tell a
knowledge gap from a failure, so never report a broken index as a gap. Defined in
[`error.rs`](crates/kaibo-core/src/error.rs), pinned by tests spread across
`query`, `sync` and `cli.rs` rather than by one contract test.

**Errors are instructions.** Every error message names the command that fixes the
situation. *(unenforced: spot-checked with `any`, never swept, and
[`status.rs`](crates/kaibo-core/src/status.rs) builds findings with no fix.)*

## Traps

**qmd's default index is not untouched, it is unwritten.** One documented
carve-out reads it, because the isolation check has to read it to prove nothing
wrote to it.

**A fixture path is not a label.** A fixture path containing the word "draft"
does not make the page a draft, and a test that passes on the path name keeps
passing after the label logic is deleted.

**A green suite is not coverage.** A mutation deleting the entire fencing
mechanism left 80 tests green. Never cite a test count as evidence that something
is held.

## Decisions in force

One line each; the rationale is in the record, and
[`docs/adr/`](docs/adr/README.md) has the full set.

- The core is AI-optional, with an explicit degradation ladder - [0002](docs/adr/0002-ai-optional-core.md)
- Answers come in two classes: grounded answer, or labeled proposal - [0003](docs/adr/0003-grounded-answers-and-labeled-proposals.md)
- qmd is isolated in a dedicated index with a single collection - [0006](docs/adr/0006-dedicated-qmd-index.md)
- Everything a skill reads is untrusted data - [0007](docs/adr/0007-read-content-is-untrusted-data.md)
- Contribution needs read access, not write - [0008](docs/adr/0008-contribution-needs-read-access-not-write.md)
- The default qmd index has no exceptions - [0010](docs/adr/0010-the-default-index-has-no-exceptions.md)
- Rust, not Go - [0011](docs/adr/0011-rust-not-go.md)
- The skills ship embedded in the binary - [0013](docs/adr/0013-ship-the-skills-inside-the-binary.md)

## Principles

Why each one is worth the friction: [`docs/concept/testing.md`](docs/concept/testing.md).

- Prove it in a test, or delete the sentence. A doc comment earns its place only
  for the non-obvious why, a trap, or a pointer to the enforcing test.
- An expected value never comes from the code under test. Write the literal.
- A test describes a situation, not a function call.
- Test-first, and keep the red-phase output.
- Tests are hermetic: no network, no real subprocess, no filesystem mutation
  outside `tempfile`. `Clock` and `Environment` are injected.
- Guardrail tests sweep, they do not enumerate.
- Prefer `pub(crate)`. Public is for what another crate genuinely needs.
