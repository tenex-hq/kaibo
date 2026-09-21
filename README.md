# kaibo

**K**nowledge **AI** **B**ack**o**ffice. A git-backed knowledge base your coding agents can
actually query and contribute to - markdown in a repo, retrieval on device, PR-reviewed like
code.

`kaibo` is the CLI. It orchestrates git, [QMD](https://github.com/tobi/qmd) and plain
markdown, and it ships the agent skills inside the binary.

> **Status: in development.** No release yet.

Organizational knowledge that would otherwise stay in experts' heads and behind their
calendars, kept as markdown in a git repo, organized by domain and by content type, and
retrieved semantically. Agents are first-class readers and writers of it, and the CLI exists
so that an agent's side of that is **one command per turn**, with a typed result, rather
than several hundred lines of prompt describing a procedure.

**[Read the overview](docs/overview.md)** for the full picture: the feature surface, how it
works, and what it guarantees.

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

`query`, `doctrine`, `domains`, `contribute plan|apply`, `sync`, `status`, `lint`,
`install`. Each takes `--json`, `--full` and `--explain`; see
[the overview](docs/overview.md#what-you-can-do-with-it) for what each one does.

## Documentation

- [Overview](docs/overview.md) - what kaibo does, the feature surface, and what it guarantees.
- [Getting Started](docs/getting-started.md) - from zero to `kaibo query` and `kaibo contribute` working.
- [AXI - the agent experience interface](docs/axi.md) - the design frame behind the verb surface.
- [QMD contract](docs/qmd-contract.md) - the exact QMD commands kaibo depends on.
- [Decision records](docs/adr/README.md) - why the guarantees are what they are, and what was deliberately not built.
- [Corpus conventions](template/CONVENTIONS.md) - the frontmatter, folder and wikilink rules `kaibo lint` checks against.
- [Evals](evals/README.md) - the caliper suites that grade the skills' activation and behaviour, and what they have found so far.

## Licence

MIT.
