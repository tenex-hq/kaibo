# Changelog

All notable changes to kaibo are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/) and this
project adheres to [Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-09-18

### Bug Fixes

- Treat an empty config value as unset at every layer
- Close two gaps found reviewing the sync verb
- Pin the mask assertion's direction and the gate's fail-safe
- Match frontmatter status literals case-insensitively
- Harden kaibo query against untrusted corpus content
- Make the qmd literal confinement guardrail recurse
- Use a generic tag example in the kebab-case rule tests
- Stage the changelog by name when cutting a release (#21)

### Build

- Set up cargo-dist release pipeline and just release recipe

### CI

- Gate mutation testing on the PR diff, drop the full sweep (#19)

### Chores

- Guard the clap definition and tighten CI

### Documentation

- Record the house rules as AGENTS.md
- Trim doc comments to why, traps, and test pointers
- Migrate public-facing documents from the internal repo (#18)

### Features

- Add the kaibo sync verb
- Add the kaibo query verb
- Add doctrine verb, a load not a question
- Add domains verb, the MOC inventory as structured data
- Add kaibo lint, a rule registry with a severity policy
- Add contribute plan and apply verbs
- Install the agent skills from the binary, and report their drift in status (#20)

### Refactor

- Extract corpus trust boundary into its own module
- Split each in-crate test module into a sibling file

### Tests

- Add opt-in qmd contract smoke check
- Add a mutation-testing gate and an end-to-end CLI harness
- Close mutants surviving in the lint rule registry
- Close mutation gaps found in the contribute module
- Cover the remaining contribute mutation gaps
- Sweep every QmdCommand constructor for --index (#22)


