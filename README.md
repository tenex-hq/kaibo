# kaibo

**Your team's engineering doctrine, one command away from every coding agent.**

Every team has a way it builds things: how tests are tiered, what a base image is
pinned to, what a pull request has to show before it merges. Most of it lives in a
few people's heads, and the rest is pasted into each repo's `AGENTS.md`, where the
copies drift apart. Agents never see the version that was actually agreed on, so
they produce work that is plausible, locally consistent, and quietly not how you do
things.

kaibo (**K**nowledge **AI** **B**ack**o**ffice) keeps that doctrine in one place,
reviewed before it counts, and gives agents one command to load it when they start
a kind of work and to research it before they decide something.

- **For the lead:** one definition of how the organisation builds, owned per
  domain, reviewed like code, with standards that can be marked binding.
- **For the engineer:** stop copying conventions from repo to repo. Write them down
  once; every agent session can read them, cite them, and propose additions.

> **Status: early release, used internally.** The verb surface is settling, and a
> breaking change is called out in the [changelog](CHANGELOG.md).

## What it looks like

An agent about to touch your test suite loads the relevant doctrine first:

```console
$ kaibo query "should integration tests hit the network"
self-heal: not needed
result: 2 hit(s)
- engineering-practices/reference/lean-hermetic-test-pyramid.md - The Lean Hermetic Test Pyramid (relevance 0.84, status current)
<<<UNTRUSTED CORPUS CONTENT path="engineering-practices/reference/lean-hermetic-test-pyramid.md">>>
...
```

And when nobody has written the answer down, kaibo says so instead of guessing:

```console
$ kaibo query "how do we version database migrations"
self-heal: not needed
result: gap, no hits
known domains: kaibo, ai-systems, observability, infrastructure, engineering-practices, ...
$ echo $?
3
```

Exit `3` means the organisation has no position yet. That gap is something to
fill, and the same agent can open the pull request that fills it with
`kaibo contribute`.

## Install

```console
brew install tenex-hq/tap/kaibo
```

Or, on macOS and Linux without Homebrew:

```console
curl -LsSf https://github.com/tenex-hq/kaibo/releases/latest/download/kaibo-installer.sh | sh
```

kaibo searches with [qmd](https://github.com/tobi/qmd), on your machine:

```console
npm install -g @tobilu/qmd@2.8.3
```

Then point it at your knowledge repo and set up the agent side:

```console
export KAIBO_REPO=your-org/knowledge   # or `repo = "..."` in ~/.kaibo/config.toml
kaibo install                          # places the Claude Code skills; they load in the next session
kaibo sync                             # clones the corpus and builds the index
```

No knowledge repo yet? [Getting Started](docs/getting-started.md#2-start-a-knowledge-repo)
shows how to start one.

## How it works

The doctrine is a git repository of plain markdown: one folder per domain, pages
typed as reference, how-to or FAQ, changes merged by pull request under
CODEOWNERS. It stays readable with nothing but a browser or `grep`.

`kaibo` is a single Rust binary that orchestrates `git`, `gh` and `qmd` so that
an agent's side of the round trip is one command per turn with a typed result:

| | |
|---|---|
| `doctrine <domain>` | Load a domain's standing guidance in one call, on entering that kind of work. |
| `query <question>` | Retrieve ranked, cited passages. The calling model does the synthesis. |
| `domains` | List what the corpus covers: domain, owner, topics. |
| `contribute plan` / `apply` | Find where a piece of knowledge belongs, then lint it and open the PR. |
| `sync`, `status` | Keep the local clone and index fresh; say what is wrong and the command that fixes it. |
| `lint` | Check the corpus against its conventions. Drops into CI. |
| `install` | Place the agent skills this binary carries. Skills and binary never drift apart. |

Every verb takes `--json`, `--full` and `--explain`. `--explain` prints the git and
qmd commands underneath and runs nothing.

## What it is not

- **Not a chatbot.** `query` retrieves; it holds no API key and calls no model.
  Your agent writes the answer, citing the pages kaibo returned.
- **Not a wiki over everything.** The corpus is prescriptive: what the team decided
  should be true, not everything anyone ever wrote down.
- **Not an enforcer.** kaibo keeps doctrine ready to read. Whether an agent reads it
  depends on the agent invoking the skill, which the [evals](evals/README.md)
  measure rather than assume.

## What it guarantees

Anyone who can merge a knowledge PR can put words in the corpus, so kaibo treats
retrieved content as data: it reaches the agent fenced, and it can never change a
command kaibo runs, a path it reads, or a flag it sets. No verb accepts a flag
naming a repo or index, git hooks in the knowledge repo never run, and no code path
discards uncommitted work. Usage telemetry stays on your machine unless you name an
OpenTelemetry collector. The full list, and the test that holds each one, is in the
[overview](docs/overview.md#what-it-guarantees).

## Documentation

- [Overview](docs/overview.md): the problem, the feature surface, and the guarantees.
- [Getting Started](docs/getting-started.md): from install to a first query and a first contribution.
- [Corpus conventions](template/CONVENTIONS.md): the folder, frontmatter and wikilink rules `kaibo lint` checks.
- [AXI](docs/axi.md): the design frame behind the verb surface.
- [Decision records](docs/adr/README.md): why the guarantees are what they are, and what was deliberately not built.
- [QMD contract](docs/qmd-contract.md): the exact qmd commands kaibo depends on.
- [Evals](evals/README.md): the suites that grade whether the skills fire and behave.
- [Contributing](CONTRIBUTING.md): commit conventions and the release flow.

## Licence

MIT.
