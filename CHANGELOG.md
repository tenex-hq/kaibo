# Changelog

All notable changes to kaibo are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/) and this
project adheres to [Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-09-21

### Bug Fixes

- Run the dependency guard without just, and mutate the otlp module
- Run the dependency guard without just, and cover the export failure
- `binding` is a boolean, so a word there binds nothing
- A word scan that cannot be mutated into standing still
- Rank and gate on rerank relevance, not qmd's blended score (#90)

### Chores

- Tag this repo's telemetry, and stop tracking agent scratch (#62)

### Documentation

- Add architecture decision records and the parking lot (#26)
- The issue tracker now lives in this repository (#60)
- Adopt the Agent Docs Standard
- State the AXI premise without the history
- Restructure AGENTS.md around the ADS sections
- Record the three layers and where kaibo invests
- Decide what the paper trail is for, and what it cannot measure
- Shape the paper trail as one wide OTel event per invocation
- Hand off the state of the conformance-contracts decision
- Mark the experiment closed, and the runner an archive
- Drop the handoff, now that it has no reader
- Cut the README to the evidence
- The normative schema, and why judgment is not in it
- A standard page carries the claim, not the argument
- Add a product overview and trim the README to a teaser
- Reconcile parked.md, axi.md and trust.rs with what shipped (#84)

### Features

- Make rule parameters configurable via config (#25)
- Add the caliper eval suites and the domain folder template (#27)
- Record why the hit list ended up empty
- The wide event shape for the paper trail
- Append one wide event per invocation
- Push the same wide event over OTLP, behind a feature
- The binding schema, decidable only
- Gate the normative schema over the corpus
- A binding page states one claim, not six
- File a binding standard, or refuse half of one
- Plan says which candidates are binding
- Record what the relevance floor withheld (#92)

### Refactor

- Consolidate the collection mask into one shared definition (#86)
- Remove the prose-style rule (#87)
- Remove the normative-atomicity lint rule (#88)
- Delete the single-variant Severity type (#89)

### Tests

- Extend the hostile-corpus fixture with an embedded frontmatter delimiter (#23)
- Cover the flags sync, query and contribute apply take end-to-end (#24)
- Cover the timestamp sign and the non-scalar fallback
- Scaffold the rubric-versus-prose experiment
- The truth set and the compiled rubric, both authored blind
- The rubric does not buy recall, and it costs 2.6x
- Replicate on a second model, and find the rubric's shape
- Measure closure, and find it real but inert
- Hold the fence scanner's three closing conditions
- Hold the scanner's closing conditions and the list-item split

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
- V0.1.0

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


