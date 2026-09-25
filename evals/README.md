# Running the evals

The case matrix lives in [CASES.md](CASES.md); this file is how to run it.

CASES.md was parked in August 2026 because `claude plugin eval` was gated behind
an early-access flag. [Caliper](https://github.com/edonadei/caliper) runs the
same matrix today, so the cases are authored here instead. Caliper's model lines
up with what CASES.md already asked for: a declared *neighbourhood* of skills,
all installed and none preloaded, an `activates:` assertion scored on its own
scoreboard separate from answer quality, and `--ablate` for the with/without arm.

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

Caliper is not installed on this machine and should not be - the Mac is
declarative (nix-darwin + Home Manager) and `pipx install` would be an
imperative install. Run it ephemerally:

```bash
uvx --from caliper-eval caliper validate evals/activation.eval.yaml

cargo build --release --locked -p kaibo
uvx --from caliper-eval caliper run evals/activation.eval.yaml --k 3 --timeout 300
uvx --from caliper-eval caliper run evals/behaviour.eval.yaml  --k 3 --timeout 300
```

**Build before every `caliper run`.** The [`stubs/kaibo`](stubs/kaibo) shim
hands off to `target/release/kaibo` in this checkout, never to a `kaibo` on
PATH, so the run grades the binary the change under test is in rather than an
installed release. A missing or stale build stops every attempt with the build
command. Stale means a file cargo compiled into the binary, an embedded skill
included, changed after it was linked; a dependency bump alone is not caught.

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
attention, so it is worth running the cheap suite on more than one - the two runs
below disagree on the same task in opposite directions.

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
  sitting in a repo", which is a shame for CASES.md case 2.
- **Case 2 (competing local evidence) is only approximated** for that reason,
  and **case 8 (reverse finding)** is not authored at all. Case 8 needs a repo
  that genuinely outruns the corpus, which a two-page stub cannot stage fairly.
- **`contribute` is measured for activation only.** Grading its output means
  letting it open a real PR. Sandboxing `gh` is the next piece of work here.

## Findings so far

**Caveat, and now for two reasons.** Every number below was measured against an
older copy of the skills, not the canonical copy in this repo that the specs now
point at. And the stub corpus has since changed: the grounded-answer page was
replaced with an invented convention in a different domain, and the task that
grades it asks a different question, so the behaviour numbers no longer describe
the suite that exists. Re-run both suites before quoting any of this again.

All runs `claude-code:claude-sonnet-5`. The k=3 numbers below are 2026-09-18.

### Activation: 36/39 = 92.3% (k=3)

Every task 3/3 except one, which was **0/3** - consistent, not noise. All four
hardened negatives held, including the OpenTelemetry-SDK-syntax trap, and all
three lane-split probes landed on the right skill.

The failure is *"Right, time to sort out how we deploy this thing."* Sonnet
fires nothing: it sees an empty working directory, judges the prompt too vague
to act on, and asks which project is meant without consulting the corpus. The
unpinned default model over-fired on the same prompt (`query` + `sync`). Both
directions on one prompt, so this is the prompt shape rather than the model: a
bare domain entry with no project in front of it. The description's worked
example, *"lets do observability"*, passes 3/3.

**This is the only finding here that is about kaibo.** Everything below is about
the harness.

### Behaviour: 85.7% full, 31.0% ablated (k=3), and both numbers are provisional

The ablation delta is real and large - the bare agent cannot produce the
corpus's reasoning about a convention nobody has published, cannot label a
proposal, cannot know the rollout page is deprecated. But neither arm should be
quoted yet, because the corpus seeding is still wrong in a way that costs the
*full* arm points. See below.

## The seeding problem

Where the fake corpus lives turns out to decide what the eval measures, and both
obvious answers are wrong. This bit three times in one day.

**Pages on disk.** The clone at `~/.kaibo/knowledge` is seeded with the page
bodies. The skill works; the staleness check passes; nothing spuriously fires
`sync`. But a bare agent with `query` ablated can simply `Read` the files, so
three tasks passed 3/3 *without* the skill and the ablated arm read 52.4%
instead of 14.3%. The ablation measured nothing.

**Pages not on disk.** Only `_index.md` is seeded - inventory,
no doctrine - and page bodies are reachable solely through `qmd get`. The
ablation is honest again. But the agent-under-test now verifies its own
citations against the clone, finds the page absent, concludes its retrieval was
hallucinated, and *retracts a correct answer* - on one attempt it stated
outright that its own citation had been fabricated. That is the full arm's 1/3
on the grounded-answer task, and it is the harness's fault, not the skill's.

It also stopped working outright once the skill went through `kaibo query`:
kaibo withholds a hit whose frontmatter it cannot read off the clone, so every
hit was withheld and every task read as a gap.

**Pages on disk when `query` is installed** (current state). `_corpus.py` writes
page bodies only if `~/.claude/skills/query/SKILL.md` exists in the attempt,
which `--ablate query` leaves out. That matches reality - the corpus is what the
skill reaches - and keeps the ablated agent empty-handed. The behaviour numbers
above predate it.

## Other harness gotchas found the hard way

- **A skill's honest failure can abort the whole run.** The `sync` skill
  correctly reported "gh is not authenticated" in the sandbox; caliper scans
  backend output for signs *Claude Code* is not logged in, matched on it, and
  killed all 13 tasks. Hence `stubs/gh`.
- **Seed before the first thing each skill touches.** `query` starts with qmd,
  `contribute` inspects the clone with git, `sync` starts with gh. Seeding from
  only one leaves the others looking at an unbootstrapped machine, which they
  fix by firing `sync` - four spurious activations across three tasks, which
  read as a kaibo over-firing defect until the transcripts were checked. Hence
  the shared `stubs/_corpus.py` and the `git` shim.
- **The judge is a full agent with filesystem access.** On one attempt it went
  looking for a cited page on the *host* disk - the attempt's HOME was long
  deleted - found nothing, and failed the task for fabricated citations. The
  affected `expect:` now tells it to judge from the transcript alone; the other
  rubrics should get the same guard.
- **The judge has a hardcoded 60s timeout** and hit it twice in one k=3 run.
  Those attempts become `judge_error` and drop out of the denominator, so a task
  can quietly report 2/3 usable rather than 3/3.

### Not yet run

Any model other than sonnet-5 since the rewrite; the activation suite ablated.
