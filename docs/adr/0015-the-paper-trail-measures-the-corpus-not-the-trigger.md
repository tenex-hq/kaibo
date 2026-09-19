---
status: accepted
---

# 0015. The paper trail measures the corpus, not the trigger

**Decided** 2026-09-19

## Context

[0014](0014-three-layers-and-where-kaibo-invests.md) records that measurement
must precede intervention at L3, the consumption trigger, because forcing
triggering through stronger prose has already been tried, failed, and failed
undetectably. The obvious candidate for that measurement is the binary writing
one record per invocation. It is not the right instrument, and the reason is
structural rather than a defect in the record shape.

The trigger question is a rate: of the occasions an agent entered a domain of
work, how often did it load doctrine first. The binary observes the numerator
only. A session in which the agent never reached for kaibo produces no record,
because kaibo never ran. **A tool cannot record its own non-invocation.** No
record shape fixes this, and a richer record makes it worse by looking like an
answer.

The predecessor instrument could see the denominator, because it read agent
transcripts rather than tool output. That is also precisely why it went blind
twice when the call path moved, and why it could not separate its own test
traffic from real use. Seeing the denominator requires standing outside the
tool, and standing outside the tool is what made it fragile. There is no
position from which both properties hold.

## Decision

**The trail is an L2 instrument.** It answers "which knowledge gap recurs, so
which page is worth writing next." That is the one decision the deleted
predecessor genuinely served, it is a corpus-quality decision, and its
denominator is internal: invocations that happened, of which some gapped.

**Activation is measured by the eval suite, not by the trail.**
`evals/activation.eval.yaml` probes the trigger directly across a fixed task
set, so the denominator is the task count and is known by construction. That
suite, not the paper trail, is what satisfies 0014's measurement prerequisite.
Its numbers are synthetic rather than field data, which is a real limitation
and a smaller one than an unknowable denominator.

Five consequences for what gets recorded follow.

**1. Two verbs, not all of them.** `query` and `doctrine` only. Their outcome
is a knowledge verdict, which is the thing being counted. `sync`, `lint` and
`install` are operational: their failures are already loud through exit codes
and stderr, and logging them adds volume without adding a decision.
`contribute` leaves a better record than a log line already, in the form of a
pull request.

**2. Recurrence needs a groupable subject.** "Which gap recurs" is
unanswerable if the only recorded subject is raw question text, because no two
phrasings group. A record therefore carries the domain, and on a gap it also
carries the nearest hit the retrieval did return, which is the same
nearest-domain signal `query` already reports to the caller
([0003](0003-grounded-answers-and-labeled-proposals.md)). Counting volume
needs neither of these, which is how a trail ends up measuring nothing.

**3. Record observable facts about the caller, never a claimed identity.**
Test traffic contaminating real use is the predecessor's second failure mode,
and a self-declared caller label reintroduces it in a new costume: anything
settable is settable by the eval harness, by CI, and by an agent composing a
command line. So the binary records what it can observe without trusting
anyone - whether it is a debug build, whether stdout is a terminal - and the
reader classifies. A fact can be wrong about what it implies. A claim can be
wrong about what it is.

**4. Local file, no reader verb, no transport.** Append-only, under `~/.kaibo/`
([0005](0005-workspace-layout.md)). A reader verb is a new verb, and 0014
presumes those harmful until argued for; the file is greppable and the
degradation ladder ([0002](0002-ai-optional-core.md)) applies to the trail as
much as to the corpus. A `kaibo log` verb earns its way in when answering
"which gap recurs" with ordinary text tools has actually become painful, and
not before.

**5. Aggregation across machines is not attempted.** Today the trail describes
one machine and one person, where a local file is complete. A second user
makes local files answer nothing aggregate, and the honest response is to
decline rather than to grow a service. A trail of questions is a trail of what
someone did not know, which makes it performance-adjacent data: transport is a
different decision requiring explicit opt-in, and never a default.

## Consequences

- #40 keeps its scope and gains three requirements from this record: the
  domain is recorded, a gap record carries the nearest hit, and the build
  profile and terminal-attachment facts are recorded.
- The log write is the binary's only routine write outside the clone, so the
  path comes from configuration and never from anything read, on the same
  terms as every other target
  ([0007](0007-read-content-is-untrusted-data.md)). Corpus-derived strings
  reaching a record are stripped of control characters exactly as output is.
- L3 intervention is unblocked by running the activation suite, not by
  shipping #40. The two stop being sequenced against each other.
- If the activation suite is later judged too synthetic to act on, the
  replacement is a transcript-side instrument with its own decision record,
  and it inherits the fragility named above rather than escaping it.
- Parked item #4 is un-parked by #40 on these terms, with the verdict half now
  scoped to gap recurrence rather than to a felt "would I have asked a human".

## Rejected

- **Treating the trail as the activation number.** It counts invocations that
  happened, so its activation rate is 100 percent by construction. Publishing
  that number would repeat the predecessor's worst moment, which was producing
  a confidently wrong figure rather than an error.
- **An OTLP exporter now.** It answers the aggregate question that has no
  second user to ask it, and it converts a local file into transport, which
  this record defers deliberately.
