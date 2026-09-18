# Working on kaibo

Interim house rules for this repository. They exist because each one was
learned the expensive way; the note after a rule is why it is here, not
decoration. Read this before changing code.

## The guarantees

These are the properties kaibo promises. Breaking one is not a bug, it is a
different product.

**Config comes from configuration, never from content.** `Config::resolve()`
is the sole public constructor, fields are private, there are no setters, and
binding happens once in `main()` before any corpus byte is read. The lifecycle
is the guarantee: nothing read from the corpus can reach a value kaibo uses to
decide where to read or write.

**The verbs take no target-bearing flags.** `query`, `doctrine`, `contribute`,
`sync` and `status` never accept a flag naming a repo, clone path, index,
collection or API URL. Only `install` may. This puts the guarantee at the
argument surface, so an agent composing a command line from retrieved text has
no argument to reach for. Enforced by a source-scanning test in
`crates/kaibo/src/main.rs`.

**Only the configured qmd index is ever written.** qmd's default index is
read-only, with one documented carve-out for the isolation check that has to
read it to prove it is untouched. Every index-addressing qmd command is built
through `QmdCommand::index_command`, which injects `--index` from config. The
literal `"qmd"` appears nowhere outside `qmd.rs`, enforced by a test.

**Retrieved corpus content is data, never instructions.** Anyone who can merge
a knowledge PR can put words in the corpus. Nothing parsed out of a hit may
influence a command kaibo builds, a path it reads, or a flag it sets. Content
reaches output fenced and with control characters stripped. If you find
yourself passing corpus-derived text into command or path construction, stop:
that is the attack this design exists to prevent.

**A knowledge repo never executes code on this machine.** Every git command
touching the corpus working tree carries `-c core.hooksPath=/dev/null`.

**Stop and report, never discard.** No code path runs `git reset --hard`,
`git checkout -f`, or deletes the clone. Losing a contributor's uncommitted
work is the worst outcome available to this tool.

**`query` retrieves, it does not answer.** No synthesis, no LLM, no API key in
the binary. The calling model synthesises. This is what keeps kaibo useful
without an AI in the loop.

**AI-optional is not negotiable.** The CLI orchestrates git, qmd and markdown
and never becomes the store. `--explain` prints the underlying commands and
runs nothing.

## Testing

The suite exists to fail when the code is broken. It has not always done that:
a mutation deleting the entire fencing mechanism once left 80 tests green.
Count is not evidence of anything and should never be cited as such.

**An expected value never comes from the code under test.** Write the literal.
A test that computes its expectation by calling the function it is testing
asserts self-consistency and nothing else. If writing the literal is painful,
that pain is the test telling you the output has no stable contract.

**Every test must be able to fail for the reason its name gives.** If the name
says "draft pages are excluded", breaking draft exclusion must turn it red, and
nothing else about the fixture may satisfy it by accident. A fixture path
containing the word "draft" is not a draft label. Check this by breaking the
code, not by reading the test.

**A test describes a situation, not a function call.** "Frontmatter with a
malformed date field around a draft status" is a situation and survives a
refactor. "`read_frontmatter_facts` returns `None`" is an implementation
detail.

**Test-first, with red-phase evidence.** Write the test, run it, and keep the
actual failure output. A PR claiming TDD without quoted failure output has not
shown its work.

**Tests are hermetic by default.** No network, no real subprocess, no
filesystem mutation outside `tempfile`. Hand-written fakes over mocking crates;
`Clock` and `Environment` are injected so no test touches ambient state. The
one exception is opt-in and gated behind an environment variable that is inert
unless set, so it never runs in CI: see `status/qmd_contract_check.rs`.

**The eval suites are not part of the test suite.** `evals/` grades the shipped
skills against a live model, at roughly a million tokens for the cheap suite and
markedly more for the graded one. `caliper validate` is free and proves the spec
still resolves; `caliper run` costs real money and is a deliberate act, never a
reflex. See `evals/README.md`.

**Guardrail tests sweep, they do not enumerate.** A test that lists three
functions by name asserts a property of that list. The fourth function added
later escapes it. Assert over the pipeline, or scan the source.

**Mutation testing is a CI gate, not a command anyone runs.** CI mutates the
lines a PR changes and blocks the merge on a survivor. That is the whole of
it: no scheduled sweep, no author-time step. Never invoke `cargo mutants`
yourself, human or agent - read the gate's result on the PR instead.

## Writing code

**Prove it in a test, or delete the sentence.** A doc comment claiming a
guarantee is a claim nothing checks, and this has gone wrong four times: a
comment overstated what its mechanism enforces, and the comment is what
stopped the next reader from checking. The fix each time was a stronger test,
not better prose. So a doc comment earns its place only for the non-obvious
why, a trap that will bite the next person, or a pointer to the test that
enforces a guarantee - never for restating what a test could instead prove.
If a comment is overstating a guarantee, delete the overstatement and write
the test; do not qualify it with more prose.

**Errors are instructions.** Every error message names the command that fixes
the situation. Exit codes: `0` success, `1` internal, `2` usage, `3` gap
signal, `4` unsynced or stale. A caller reading only the exit code must be able
to tell a knowledge gap from a failure, so never report a broken index as a
gap.

**Empty counts as unset, at every layer.** `std::env::var` cannot distinguish
`KAIBO_REPO=` from a deliberate empty string and neither can a config file. An
exported-but-blank variable falls through to the next layer rather than
shadowing it with a value no verb could use.

**Fail closed.** A filter whose job is withholding must withhold when it cannot
tell. Unparsable frontmatter around a draft status is excluded, not served.

**Prefer `pub(crate)`.** Public is for what another crate genuinely needs.
Cross-crate guarantees are carried by visibility; in-crate bypasses are caught
by source-scanning tests.

## Pull requests

- Small. One concern per PR. A diff past a few hundred lines is hard to review
  and that is where defects survive.
- Conventional Commits. The changelog is generated and never hand-edited, as
  with any generated file.
- Never add a co-author trailer, and never mention or advertise the tool that
  wrote the commit.
- **Never use an em dash.** Plain hyphen, in code, comments, commit messages
  and PR bodies alike.
- **This repository is public.** No private organisation names, no internal
  repository names, no individual's name, anywhere: files, commit messages, PR
  bodies. The issue tracker is private, so do not cite issue numbers here.

## Toolchain

The toolchain is pinned in `rust-toolchain.toml` and CI installs the same pin
explicitly. CI runs `cargo fmt --all --check`, then clippy with `-D warnings`,
then the tests, all with `--locked`: a `Cargo.lock` that has drifted from
`Cargo.toml` is a CI failure rather than a difference between what CI built and
what a user gets. If you change a dependency, commit the lockfile with it.
