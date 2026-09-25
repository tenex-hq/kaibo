# Running the evals

The case matrix lives in [CASES.md](CASES.md); this file is how to run it.

[Caliper](https://github.com/edonadei/caliper) runs the matrix. Its model lines
up with what CASES.md asks for: a declared *neighbourhood* of skills, all
installed and none preloaded, an `activates:` assertion scored on its own
scoreboard separate from answer quality, and `--ablate` for the with/without arm.

Past runs, with their numbers and what they found, are dated records in
[`docs/records/`](../docs/records/).

## The skill neighbourhood: the skills in this repo

Both specs declare their `skills:` neighbourhood as
`../crates/kaibo-core/skills/query/SKILL.md` and its `contribute`/`sync`
siblings. Caliper resolves a bare skill path against the **spec file's own**
directory, so from `evals/` that is `crates/kaibo-core/skills/` - the skills
embedded in the CLI binary, at the revision being tested.

That is the whole of it. No second copy to keep in step, no symlink, no sibling
checkout that can go missing: the evals grade the skills that ship, and a change
to a `description` is graded by the same commit that makes it.

## Two suites, different price

| Suite | Tasks | Judge | Cost | When |
|---|---|---|---|---|
| [`activation.eval.yaml`](activation.eval.yaml) | 13 | none - trigger probes only | ~1M tokens, ~5 min at k=1 | every change to a `description` |
| [`behaviour.eval.yaml`](behaviour.eval.yaml) | 7 | LLM autorater | markedly higher | before a release |

The split is CASES.md's own: cases 1-3 and 5-6 are answered by the tool-call
sequence alone, so they carry no `expect:` and skip the judge entirely. Cases 4
and 7 are judgement calls about the answer's content and need a grader.

## Commands

Caliper needs no install; run it ephemerally with `uvx`:

```bash
uvx --from caliper-eval caliper validate evals/activation.eval.yaml

cargo build --release --locked -p kaibo
uvx --from caliper-eval caliper run evals/activation.eval.yaml --k 3 --timeout 300
uvx --from caliper-eval caliper run evals/behaviour.eval.yaml  --k 3 --timeout 300
```

**Build before every `caliper run`.** The [`stubs/kaibo`](stubs/kaibo) shim
hands off to `target/release/kaibo` in this checkout, never to a `kaibo` on
PATH, so the run grades the binary the change under test is in rather than an
installed release. A missing build stops every attempt with the build command;
a stale one is not detected, so rebuild after any change to `crates/`.

`--timeout 300` is not optional. A skill that fires still executes its body, and
`contribute` does git and gh work against a repo that is not there. At the 120s
default those attempts are marked UNUSABLE and drop out of the denominator,
which reads as a pass rate over fewer tasks rather than as a failure.

### Pinning the model

**Always pass `--model`.** With it omitted, caliper records `model: null` and the
attempt runs on whatever the local `claude` CLI happens to default to that day -
the run is unreproducible and not comparable to any other. The flag takes
`backend:model`:

```bash
--model claude-code:claude-sonnet-5
--model claude-code:claude-opus-5
--judge-model claude-code:claude-haiku-4-5-20251001   # graded suite only
```

The backend prefix matters: a bare alias like `sonnet` is read as a backend name.

Activation is a property of the *description* competing for a given model's
attention, so it is worth running the cheap suite on more than one: the same
prompt can under-fire on one model and over-fire on another.

**Pass `--no-user-customizations`.** Without it, attempts load the MCP servers
and connectors the operator's own `claude` CLI loads, so the score depends on
whose machine ran it.

### The ablation arm

The number that justifies the plugin's context cost is not "it answered well",
it is "it answered better than the bare agent would have". Run the same spec
with a skill removed and diff:

```bash
uvx --from caliper-eval caliper run evals/behaviour.eval.yaml --k 3 --timeout 300 --ablate query
uvx --from caliper-eval caliper compare \
  .caliper/results/behaviour/<full-run>.json \
  .caliper/results/behaviour/<ablated-run>.json
```

Ablation is a property of the tasks, not of a code change, so run the ablated
arm once and keep re-diffing new runs against it.

## The fake corpus (`stubs/qmd`)

Every caliper attempt runs in a fresh throwaway HOME with no session history, so
the real `~/.kaibo/knowledge` clone and the `kaibo` qmd index are not there.
Both specs put [`stubs/qmd`](stubs/qmd) on PATH via `sandbox.extra_path`, ahead
of the real binary.

The skills reach the corpus through `kaibo`, not qmd, and kaibo is real: it
parses qmd's `--format json --explain` output against
[`docs/qmd-contract.md`](../docs/qmd-contract.md), floors on
`explain.rerankScore`, and reads each hit's frontmatter off the clone. So the
stub speaks that contract, and a [`stubs/kaibo`](stubs/kaibo) shim seeds the
clone before handing off to the workspace build, because kaibo checks the clone
on disk before it runs anything.

The stub serves a deliberately lopsided corpus: a handful of domains are
covered (schema registry, collector processor ordering, span naming, sampling,
rollout, secrets), everything else returns zero hits. That makes both branches
of the query skill reachable with a known-in-advance right answer - the grounded
path and the gap path - which is what lets case 4 be graded at all. The stub
also seeds the root MOC into the attempt's HOME on first call, since the skill
reads `~/.kaibo/knowledge/_index.md` on the no-hits path. It is written in the
section format [`template/CONVENTIONS.md`](../template/CONVENTIONS.md) fixes,
bold `- **owner:**` bullets and all, because that is the only shape kaibo's MOC
parser reads; anything else leaves `kaibo doctrine` reporting every domain as
owned by no one with no topics.

The grounded-answer page is **invented**, not a convention anyone has written
down: invented tools, an invented rationale, an arbitrary threshold. That is
load-bearing for the ablation. A bare agent can talk its way to a plausible
answer about a real vendor comparison from general knowledge alone, scoring the
ablated arm points it did not earn; it cannot guess a convention that exists
only in `stubs/_corpus.py`.

The stub refuses to run unless `$HOME` is a `caliper-*` sandbox. It writes into
`$HOME/.kaibo/knowledge`, and run by hand on a workstation it would overwrite
the developer's real clone.

Grading against a stub grades the *skill*, not the corpus. A regression in the
knowledge monorepo's content will not show up here, and should not - that is a
different measurement.

## Known limits

- **`setup:` cannot reach the attempt's HOME.** Caliper runs `setup:` on the
  host, in the invoking shell, before the isolated home exists; the agent then
  runs with both HOME and cwd set to that temp dir. So a fixture cannot be
  placed in the agent's working directory. The tasks that need one write it to a
  fixed absolute path (`/tmp/caliper-kaibo/...`) and name that path in the
  prompt, with a `cleanup:` to match. It costs the realism of "the agent is
  sitting in a repo".
- **Case 2 (competing local evidence) is only approximated** for that reason,
  and **case 8 (reverse finding)** is not authored. Case 8 needs a repo that
  genuinely outruns the corpus, which a two-page stub cannot stage fairly.
- **`contribute` is measured for activation only.** Grading its output means
  letting it open a real PR, and `gh` is not sandboxed for that.

## Where the page bodies are seeded

Where the fake corpus lives decides what the eval measures, and both obvious
answers are wrong.

- **Always on disk.** A bare agent with `query` ablated can `Read` the files, so
  it passes tasks without the skill and the ablation measures nothing.
- **Never on disk**, reachable only through qmd. kaibo withholds a hit whose
  frontmatter it cannot read off the clone, so every task reads as a gap. And an
  agent that checks its own citations against the clone finds the page absent,
  concludes its retrieval was hallucinated, and retracts a correct answer.

So `_corpus.py` writes page bodies only when `~/.claude/skills/query/SKILL.md`
exists in the attempt, which `--ablate query` leaves out. That matches reality -
the corpus is what the skill reaches - and keeps the ablated agent away from the
fake corpus. It does nothing about a real one; see the traps below.

## Harness traps

- **A skill's honest failure can abort the whole run.** The `sync` skill
  reporting "gh is not authenticated" matches caliper's scan for signs that
  *Claude Code* is not logged in, and caliper kills every task. Hence
  `stubs/gh`.
- **Seed before the first thing each skill touches.** `query` starts with qmd,
  `contribute` inspects the clone with git, `sync` starts with gh. A skill that
  finds an unbootstrapped machine fixes it by firing `sync`, which reads as a
  kaibo over-firing defect until the transcripts are checked. Hence the shared
  `stubs/_corpus.py` and the `git` shim.
- **The judge is a full agent with filesystem access.** It can go looking for a
  cited page on the *host* disk, long after the attempt's HOME was deleted, find
  nothing, and fail the task for fabricated citations. A rubric guards against
  that by telling it to judge from the transcript alone; only *Answer is the
  delta* does.
- **The judge has a hardcoded 60s timeout.** A timed-out attempt becomes
  `judge_error` and usually drops out of the denominator, so a task can quietly
  report 2/3 usable rather than 3/3. Caliper has also counted one as a **pass**,
  so read `autorater_reasoning` before trusting a pass.
- **The sandbox isolates HOME, not the disk.** An ablated agent with nothing in
  its own HOME can search the host, find the operator's real
  `~/.kaibo/knowledge`, and answer from it. On a workstation with a clone, the
  ablated number includes the operator's own corpus.
- **Ablating the skill does not ablate the CLI.** `stubs/kaibo` is on PATH in
  both arms, so a bare agent that guesses the name gets a seeded clone and a
  working `kaibo query`. Whether that is a leak depends on the question: it is
  if the ablation asks "what is kaibo worth", it is not if it asks "what is the
  skill worth on top of the installed binary".
- **Results land in the working directory.** Caliper writes every run's JSON,
  transcripts included, to `.caliper/`. Transcripts quote whatever the agent
  read, a host corpus included, so the directory is gitignored.
