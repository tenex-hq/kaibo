# What kaibo is, and what it does

> A high-level tour of the product: the problem, the feature surface, how it
> works, and what it guarantees. For the mechanics, see
> [Getting Started](getting-started.md); for the reasoning, see the
> [decision records](adr/README.md).

---

## In one line

**Your team's hard-won knowledge, as markdown in a git repo, that coding agents
can query with citations and contribute to by pull request.**

## The problem

The knowledge that decides whether work goes well is mostly not written down. It
is in a handful of people's heads, in closed Slack threads, and behind their
calendars. Coding agents cannot reach any of it, so they answer from general
training and from whatever repo happens to be open. They produce something
plausible, locally consistent, and quietly at odds with how the organisation
decided to do things.

The usual fix is retrieval over whatever documents exist. That inherits the
problem it was meant to solve: the wiki nobody updated, the exported chat logs,
the document graveyard. Pointing a good retriever at a bad corpus returns
confident answers from stale pages.

## The idea

Two moves, and the second is the one that matters.

**Make the corpus a product.** Knowledge lives as markdown in one git
repository, organised by domain and by content type, reviewed under CODEOWNERS
like code, and gated by a linter. It is *prescriptive*: it records what the
organisation decided should be true, not everything that was ever written down.
Pages are atomic, carry a `status`, and can be marked binding.

**Make agents first-class readers and writers of it.** A single CLI turns the
whole round trip into one command per agent turn, with a typed result. No
several-hundred-line prompt describing a procedure; no agent assembling a git
and index pipeline out of instructions.

The corpus stays plain markdown throughout. Eyes, `grep` and a browser work on
it with no binary present, which is a hard constraint rather than a nice
property.

---

## What you can do with it

### Read

| | |
|---|---|
| **`kaibo doctrine <domain>`** | Load a domain's standing guidance in one call: its section of the root map plus every `current` reference page. Not a question, a load - fired on entering a domain of work. |
| **`kaibo query <question>`** | Retrieve ranked, cited evidence for a question. Returns passages and sources; the calling model does the synthesis. |
| **`kaibo domains`** | List the domain inventory as structured data: domain, owner, topics, summary. How an agent discovers what the corpus even covers. |

`query` holds no API key and makes no model call of its own. That separation is
what keeps the corpus usable when the AI layer is unavailable.

### Write

| | |
|---|---|
| **`kaibo contribute plan`** | Surface placement candidates for a piece of knowledge: which domain, which existing page to append to, or a new page. Read-only; it never writes and never decides for you. |
| **`kaibo contribute apply`** | Write the page, lint-gate it, check its references with reflock when installed, branch, commit, push (directly or via a verified fork), open the PR, and watch CI. One command, the whole round trip. |

Contribution needs read access to the knowledge repo, not write access. The
fork route is the default for contributors who do not have the commit bit.

### Keep it honest

| | |
|---|---|
| **`kaibo lint`** | Run the rule registry over the corpus, or over given paths. Any violation fails the run. Drops straight into CI. |
| **`kaibo status`** | Is this healthy, and what is the one command that fixes it. Read-only. |
| **`kaibo sync`** | Clone or pull the corpus, refresh the index and embeddings. Also the first-run bootstrap. Idempotent. |
| **`kaibo install`** | Place the agent skills the binary carries where the agent finds them. Skills and binary are one artefact at one version, so they cannot drift apart. |

Every verb takes `--json` for a machine-readable result, `--full` to widen a
deliberately narrow default schema, and `--explain` to print the underlying git
and index commands without running anything.

---

## The features behind the verbs

### Epistemics are fields, not adjectives

The failure mode of a knowledge system is unwarranted confidence, not poor
recall. So confidence lives in the wire format instead of in an instruction to
be careful. Every result carries each page's `status` (`draft`, `current`,
`deprecated`), the age of the corpus, and whether the corpus actually answered.

Exit codes make that state machine-readable:

| code | meaning |
|---|---|
| `0` | answered |
| `2` | usage error |
| `3` | **gap** - the corpus has no position on this |
| `4` | corpus unsynced, or stale past the threshold |

Exit `3` is the interesting one. "The organisation has no position on this, the
nearest domain is X" becomes something a script, a hook or a CI job can act on,
rather than something an agent has to notice and then be honest about. A broken
index is never reported as a gap.

### Binding standards, checked locally

Normativity is an axis, not a content type. Any reference page can declare
itself binding, with a severity (`must` or `should`) and the actions it applies
to: writing a file, writing a commit message, running a command, answering in
prose, shipping, recording a decision. Keying on the action the caller is about
to take means the caller never has to suspect a gap first.

A binding page may carry a block of decidable checks - forbid a pattern, require
a pattern, require one thing wherever another appears, forbid a path - that a
client evaluates against the artifact before it acts. The checks produce facts:
which rule, where, what matched. No model involved, works offline.

The schema deliberately stops there. A second, model-judged check species was
specified and then measured against the same rules written as plain prose: it
bought no extra recall at two to three times the cost, so it is not in the
vocabulary. ([ADR 0017](adr/0017-the-conformance-schema-is-decidable-only.md).)

### Self-healing and a paper trail

`query` and `doctrine` notice a missing, stale or unindexed corpus and sync
themselves rather than lecturing the caller. Errors name the exact next command,
or do not happen at all.

Each invocation appends one wide event to a local paper trail, so corpus gaps
become a measured property of the system rather than an archaeology exercise.
The binary writes it, never the agent, so a read path never holds a write
capability. An optional OTLP exporter ships the same event to any OpenTelemetry
backend.

### Agent skills in the box

The skills that tell an agent when to reach for kaibo ship embedded in the
binary and are placed by `kaibo install`. They are graded by an eval suite in
the repo: one cheap suite that probes whether the skills activate at the right
moment, one expensive suite that grades answer behaviour before a release.

---

## How it works

```
  agent  ──  kaibo (CLI)  ──┬── git         clone, pull, branch, commit, PR
                            ├── gh          auth, fork, PR, CI watch
                            └── qmd         index, embed, semantic search
                                             │
                             ~/.kaibo/knowledge/   your corpus, plain markdown
```

kaibo orchestrates three tools it does not replace. It is never the store. The
retrieval layer is deliberately commodity and deliberately swappable: effort
spent making kaibo a better retriever would be effort spent on the half with no
moat. The corpus, and whether an agent loads it at the moment that matters, are
where the work goes. ([ADR 0014](adr/0014-three-layers-and-where-kaibo-invests.md).)

---

## What it guarantees

These are product boundaries, not implementation details. Breaking one would be
a different product.

- **Configuration is bound before content is read.** Where the corpus lives and
  where contributions go are resolved once at startup, before a single corpus
  byte is parsed. A repo name written inside a retrieved page has nothing left
  to influence.
- **Retrieved content is data, never instructions.** Anything parsed out of a
  hit reaches the caller fenced, with control characters stripped, and can never
  change a command kaibo runs, a path it reads, or a flag it sets.
- **No verb takes a target-bearing flag.** None of the verbs accept a flag
  naming a repo, clone path, index, collection or API URL. An agent composing a
  command line out of retrieved text has no argument to reach for.
- **A knowledge repo never executes code on your machine.** Every git command
  against the corpus runs with hooks disabled.
- **Nothing is ever discarded.** No code path resets, force-checks-out, or
  deletes the clone. Uncommitted work stops a verb; it never loses it.
- **Only kaibo's own index is written.** Its index and collection are isolated;
  personal collections in the default index are never touched.
- **Fail closed.** A filter whose job is withholding withholds when it cannot
  tell. A page with unparsable frontmatter is excluded, not served.
- **AI-optional, always.** Plain markdown, a thin orchestrator, and `--explain`
  to see the commands underneath. A knowledge base only a binary could read
  would be a worse product.

---

## What it needs

- A git repository for the corpus, public or private.
- [`gh`](https://cli.github.com/), authenticated with read access to it.
- [`qmd`](https://github.com/tobi/qmd), at the version the
  [contract](qmd-contract.md) is verified against.
- `KAIBO_REPO`, or a `repo` key in `~/.kaibo/config.toml`. There is no
  compiled-in default.

Lint rules are reparameterisable from that same config file: which frontmatter
keys are required, which `status` values are accepted, how a folder maps to a
content type, the tag pattern, and which rules are off entirely. None of it can
come from the knowledge repo itself.

A single Rust binary, no runtime, no daemon, no server. Skills install into the
agent in one command.

---

## Status

In development, no release yet. Follow along in the
[decision records](adr/README.md), which also record what was deliberately not
built and why.
