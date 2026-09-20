# The fixed standard set

Five standards, taken verbatim from
[`docs/concept/testing.md`](../../../docs/concept/testing.md). Both arms receive
exactly these five and nothing else. The ids below exist so the truth set, the
rubric and the grader can refer to the same rule; the prose arm is handed the
headings and bodies without them, because a corpus page carries a title, not an
id.

Two sections of the source document are deliberately excluded. "Test-first, with
red-phase evidence" and "Mutation testing is a CI gate" cannot be judged from a
file alone - they are claims about how the file came to exist. "The eval suites
are not the test suite" is about `evals/`, not about a test. Judging on them
would measure the artifact selection, not the arms.

All five are judgment species. None is decidable by regex, which is the whole
point: the decidable half of a contract produces facts, and comparing a fact
against prose is meaningless (issue #33, "Scope: judgment items only").

---

## t1 - An expected value never comes from the code under test

Write the literal. A test that computes its expectation by calling the function
it is testing asserts self-consistency and nothing else - it stays green through
any change that is internally consistent, including a wrong one.

If writing the literal is painful, that pain is the test telling you the output
has no stable contract. Fix the contract rather than the test.

## t2 - Every test must be able to fail for the reason its name gives

If the name says "draft pages are excluded", breaking draft exclusion must turn
it red, and nothing else about the fixture may satisfy it by accident. A fixture
path containing the word "draft" is not a draft label, and a test that passes
because of the path name will keep passing after the label logic is deleted.

Check this by breaking the code, not by reading the test.

## t3 - A test describes a situation, not a function call

"Frontmatter with a malformed date field around a draft status" is a situation:
it survives the refactor that renames the function. "`read_frontmatter_facts`
returns `None`" is an implementation detail, and it has to be rewritten every
time the implementation moves, which is how a suite ends up describing code
nobody has read in a year.

## t4 - Tests are hermetic by default

No network, no real subprocess, no filesystem mutation outside `tempfile`.
Hand-written fakes over mocking crates; `Clock` and `Environment` are injected
so no test touches ambient state. A suite that reads the wall clock or the real
environment fails on the machine that has a different one, and the failure is
attributed to the change rather than to the suite.

The one exception is opt-in behind an environment variable that is inert unless
set, so it never runs in CI.

## t5 - Guardrail tests sweep, they do not enumerate

A test that lists three functions by name asserts a property of that list, not
of the codebase. The fourth function added later escapes it silently, and the
guardrail reports success while the hole it was written to close is open. Assert
over the pipeline, or scan the source.
