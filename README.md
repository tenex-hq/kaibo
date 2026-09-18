# kaibo

**K**nowledge **AI** **B**ack**o**ffice. A git-backed knowledge base your coding agents can
actually query and contribute to - markdown in a repo, retrieval on device, PR-reviewed like
code.

`kaibo` is the CLI. It orchestrates git, [QMD](https://github.com/tobi/qmd) and plain
markdown, and it ships the agent skills inside the binary.

> **Status: in development.** No release yet.

## What it is

Organizational knowledge that would otherwise stay in experts' heads and behind their
calendars, kept as markdown in a git repo, organized by domain and by content type
(reference / how-to / faq), and retrieved semantically. Agents are first-class readers and
writers of it.

The CLI exists so that an agent's side of that is **one command per turn**, with a typed
result, rather than several hundred lines of prompt describing a procedure.

## Design

Three things make it what it is.

**Epistemics are fields, not adjectives.** The failure mode of a knowledge system is
unwarranted confidence, not poor recall. So every result carries page `status`, corpus age,
and whether the corpus actually answered - and exit code `3` means *gap*, which a script or
a CI job can act on without an agent having to notice and be honest about it.

**Configuration is bound before content is read.** Where the corpus lives, and where
contributions are written, come from config resolved in `main()` - before any corpus byte is
parsed. A repo name inside a retrieved page has nothing left to influence. Retrieved content
is returned pre-fenced as untrusted third-party data.

**AI-optional, always.** The corpus stays fully usable with eyes, grep and BM25 alone. The
CLI is a thin orchestrator, never the store; `--explain` prints the underlying commands. A
knowledge base only a binary could read would be a different, worse product.

## Install

Not yet released. When it is:

```console
brew install tenex-hq/tap/kaibo
kaibo install     # writes the agent skills; they load in the next session
kaibo sync        # clone the corpus, build the index
```

`kaibo install` writes a skills-directory plugin to `~/.claude/skills/kaibo/`, so the skills
and the binary are one artefact at one version and cannot drift apart. No marketplace step.

## Verbs

| | |
|---|---|
| `kaibo query <question>` | retrieve, cited. Does not synthesise - the calling model does |
| `kaibo doctrine <domain>` | load a domain's standing guidance on entering it |
| `kaibo contribute plan\|apply` | classify, place, write the page, open the PR |
| `kaibo sync` | clone or pull the corpus, reindex, embed |
| `kaibo status` | is this healthy, and what is the one command to fix it |
| `kaibo lint` | check pages against the conventions the binary implements |
| `kaibo install` | place the agent skills |

`query` retrieves; it never calls a model and holds no API key. That separation is what keeps
the whole thing usable when the AI layer is unavailable.

## Documentation

- [Getting Started](docs/getting-started.md) - from zero to `kaibo query` and `kaibo contribute` working.
- [AXI - the agent experience interface](docs/axi.md) - the design frame behind the verb surface.
- [QMD contract](docs/qmd-contract.md) - the exact QMD commands kaibo depends on.
- [Decision records](docs/decisions/README.md) - why the guarantees are what they are, and what was deliberately not built.
- [Corpus conventions](template/CONVENTIONS.md) - the frontmatter, folder and wikilink rules `kaibo lint` checks against.

## Licence

MIT.
