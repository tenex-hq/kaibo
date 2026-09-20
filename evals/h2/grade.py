#!/usr/bin/env python3
"""Grade an H2 run, blind.

Three phases, run in order against one `runs/<stamp>/` directory:

  normalise  rewrite every answer from both arms into one common finding shape,
             so a grader cannot tell the arms apart by their formatting
  grade      match the shuffled, anonymised findings against the truth set
  report     compute recall, false positives, variance and token cost per arm

The split exists because arm B answers in JSON with rule ids and arm A answers
in prose. A grader reading raw output can tell them apart at a glance, and a
grader who knows which arm it is reading is not a blind grader.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import random
import statistics
import subprocess
import sys

HERE = pathlib.Path(__file__).parent
TRUTH = HERE / "truth" / "truth-set.json"

FINDING_SCHEMA = {
    "type": "object",
    "properties": {
        "findings": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "fn": {"type": ["string", "null"]},
                    "lines": {"type": ["array", "null"], "items": {"type": "integer"}},
                    "claim": {"type": "string"},
                    "cited_rule": {"type": ["string", "null"]},
                },
                "required": ["fn", "lines", "claim", "cited_rule"],
            },
        }
    },
    "required": ["findings"],
}


def claude(prompt: str, model: str, schema: dict | None = None) -> dict:
    cmd = [
        "claude", "-p", "--safe-mode", "--model", model, "--tools", "",
        "--strict-mcp-config", "--no-session-persistence",
        "--permission-prompts", "none", "--output-format", "json",
    ]
    if schema:
        cmd += ["--json-schema", json.dumps(schema)]
    proc = subprocess.run(cmd, input=prompt, capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        sys.exit(f"claude failed: {proc.stderr.strip()[:500]}")
    envelope = json.loads(proc.stdout)
    try:
        return json.loads(envelope["result"])
    except (KeyError, json.JSONDecodeError):
        sys.exit(f"unparsable model answer: {str(envelope.get('result'))[:500]}")


def normalise(run: pathlib.Path, model: str) -> None:
    manifest = json.loads((run / "manifest.json").read_text())
    out = []
    for rec in manifest["records"]:
        if rec.get("error") or not rec.get("answer"):
            print(f"skipping {rec['tag']}: {rec.get('error')}", file=sys.stderr)
            continue
        print(f"normalising {rec['tag']}", file=sys.stderr)
        res = claude(
            f"""Below is one reviewer's report on a Rust test file. Extract every distinct \
problem the reviewer **asserts is present**.

Rules for extraction:
- One entry per distinct problem. Do not merge two problems, do not split one.
- Copy the reviewer's claim faithfully and neutrally in one sentence. Do not \
evaluate it, do not soften it, do not add anything the reviewer did not say.
- **Ignore everything the reviewer reports as fine**: anything marked compliant, \
passing, not applicable, or "no violation found" is not a finding.
- `fn` is the test function the reviewer points at, or null if it names none.
- `lines` are the line numbers the reviewer cites, or null.
- `cited_rule` is the rule identifier or rule name the reviewer attributes the \
problem to, copied verbatim, or null if it attributes none.
- If the reviewer asserts no problems at all, return an empty list.

# The report

{rec['answer']}""",
            model,
            FINDING_SCHEMA,
        )
        for i, f in enumerate(res.get("findings", [])):
            out.append({**f, "arm": rec["arm"], "artifact": rec["artifact"],
                        "repeat": rec["repeat"], "src": f"{rec['tag']}#{i}"})
    (run / "findings.json").write_text(json.dumps(out, indent=2))
    print(f"\n{len(out)} findings -> {run / 'findings.json'}", file=sys.stderr)


def grade(run: pathlib.Path, model: str, seed: int) -> None:
    truth = json.loads(TRUTH.read_text())
    findings = json.loads((run / "findings.json").read_text())

    rng = random.Random(seed)
    keyed = list(findings)
    rng.shuffle(keyed)
    # The key file is what makes this blind: the grader is handed opaque ids and
    # never the arm, and the mapping back is written where the grader cannot see.
    key = {}
    for n, f in enumerate(keyed, 1):
        f["opaque"] = f"f{n:03d}"
        key[f["opaque"]] = {"arm": f["arm"], "src": f["src"]}
    (run / "grading-key.json").write_text(json.dumps(key, indent=2))

    verdicts = {}
    for artifact, entry in sorted(truth["artifacts"].items()):
        mine = [f for f in keyed if f["artifact"] == artifact]
        if not mine:
            continue
        print(f"grading {artifact} ({len(mine)} findings)", file=sys.stderr)
        defects = [{"id": d["id"], "rule": d["rule"], "span": d["span"],
                    "description": d["what_changed"], "why": d["why_it_violates"]}
                   for d in entry["defects"]]
        anon = [{"id": f["opaque"], "fn": f["fn"], "lines": f["lines"],
                 "claim": f["claim"]} for f in mine]
        decoys = [{"id": x["id"], "resembles_rule": x["resembles_rule"],
                   "span": x["span"], "why_it_is_compliant": x["why_it_is_compliant"]}
                  for x in entry.get("decoys", [])]
        res = claude(
            f"""You are grading reviews of the Rust test file `{artifact}` against a \
hand-built truth set. Several reviewers each reported problems; their reports have \
been rewritten into one common shape, so you cannot and should not try to work out \
who wrote which. Judge every finding on its own terms.

# The truth set: the defects actually present in this file

{json.dumps(defects, indent=2)}

Rules this file was checked against and found clean: {json.dumps(entry.get("verified_clean", []))}

# The decoys: spans that resemble a violation and are not one

{json.dumps(decoys, indent=2)}

These are compliant on the rule's own terms. A finding that flags one is a false
positive, and a more interesting kind than a random one, so record which decoy it
landed on in `matched_decoy`.

# The findings to grade

{json.dumps(anon, indent=2)}

For each finding, decide which truth-set defect it identifies, if any.

- A finding **matches** a defect when it points at the same problem in the same \
place. It need not cite the right rule, use the same words, or give the same \
reason: credit what it describes.
- A finding that describes a real problem at a defect's location but misreads \
*why* it is a problem still matches. Note it in `reason`.
- `matched_defect` is null when the finding corresponds to no defect in the truth \
set. That is a false positive, and it is the expensive column in this experiment - \
do not be generous to avoid recording one.
- A vague finding that could be stretched to cover a defect does not match. \
Stretching inflates recall for whichever reviewer wrote most.
- `matched_decoy` names the decoy a false positive landed on, or is null. A \
finding can never match both a defect and a decoy.

Answer with one entry per finding id, in the schema given.""",
            model,
            {
                "type": "object",
                "properties": {
                    "gradings": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {"type": "string"},
                                "matched_defect": {"type": ["string", "null"]},
                                "matched_decoy": {"type": ["string", "null"]},
                                "reason": {"type": "string"},
                            },
                            "required": ["id", "matched_defect", "matched_decoy", "reason"],
                        },
                    }
                },
                "required": ["gradings"],
            },
        )
        for g in res["gradings"]:
            verdicts[g["id"]] = g
    (run / "gradings.json").write_text(json.dumps(verdicts, indent=2))
    print(f"\n{len(verdicts)} graded -> {run / 'gradings.json'}", file=sys.stderr)


def report(run: pathlib.Path) -> None:
    truth = json.loads(TRUTH.read_text())
    manifest = json.loads((run / "manifest.json").read_text())
    findings = {f["src"]: f for f in json.loads((run / "findings.json").read_text())}
    key = json.loads((run / "grading-key.json").read_text())
    gradings = json.loads((run / "gradings.json").read_text())

    all_defects = {d["id"]: (a, d) for a, e in truth["artifacts"].items()
                   for d in e["defects"]}
    # One attempt is one (arm, artifact, repeat): the unit a recall number is over.
    attempts: dict[tuple, dict] = {}
    for opaque, meta in key.items():
        f = findings[meta["src"]]
        at = attempts.setdefault((f["arm"], f["artifact"], f["repeat"]),
                                 {"hits": set(), "fp": 0, "decoys": set()})
        g = gradings.get(opaque, {})
        if g.get("matched_defect"):
            at["hits"].add(g["matched_defect"])
        else:
            at["fp"] += 1
            if g.get("matched_decoy"):
                at["decoys"].add(g["matched_decoy"])

    print(f"# H2 run {run.name}\n")
    print(f"model {manifest['model']}, k={manifest['k']}, "
          f"{len(all_defects)} defects in the truth set\n")
    print("| arm | recall mean | recall per repeat | false positives mean | "
          "decoy hits | input tok | output tok | cost USD |")
    print("|---|---|---|---|---|---|---|---|")
    for arm in ("A", "B"):
        per_artifact_repeat = []
        fps = []
        decoy_hits = 0
        for (a, art, rep), at in sorted(attempts.items()):
            if a != arm:
                continue
            total = len(truth["artifacts"][art]["defects"])
            per_artifact_repeat.append(len(at["hits"]) / total if total else 0.0)
            fps.append(at["fp"])
            decoy_hits += len(at["decoys"])
        recs = [r for r in manifest["records"] if r["arm"] == arm]
        if not per_artifact_repeat:
            print(f"| {arm} | no data | | | | | | |")
            continue
        sd = statistics.stdev(per_artifact_repeat) if len(per_artifact_repeat) > 1 else 0.0
        print(f"| {arm} | {statistics.mean(per_artifact_repeat):.2f} "
              f"| sd {sd:.3f} over {len(per_artifact_repeat)} attempts "
              f"| {statistics.mean(fps):.2f} | {decoy_hits} "
              f"| {sum(r['input_tokens'] for r in recs)} "
              f"| {sum(r['output_tokens'] for r in recs)} "
              f"| {sum(r['cost_usd'] for r in recs):.4f} |")

    print("\n## Per defect, how many attempts found it\n")
    print("| defect | rule | difficulty | arm A | arm B |")
    print("|---|---|---|---|---|")
    for did, (art, d) in sorted(all_defects.items()):
        counts = {}
        for arm in ("A", "B"):
            n = sum(1 for (a, ar, _), at in attempts.items()
                    if a == arm and ar == art and did in at["hits"])
            tot = sum(1 for (a, ar, _) in attempts if a == arm and ar == art)
            counts[arm] = f"{n}/{tot}" if tot else "-"
        print(f"| {did} | {d['rule']} | {d.get('difficulty', '?')} "
              f"| {counts['A']} | {counts['B']} |")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("phase", choices=["normalise", "grade", "report"])
    ap.add_argument("run", type=pathlib.Path)
    ap.add_argument("--model", default="claude-sonnet-5")
    ap.add_argument("--seed", type=int, default=33)
    args = ap.parse_args()
    if not args.run.is_dir():
        sys.exit(f"no run at {args.run}")
    {"normalise": lambda: normalise(args.run, args.model),
     "grade": lambda: grade(args.run, args.model, args.seed),
     "report": lambda: report(args.run)}[args.phase]()


if __name__ == "__main__":
    main()
