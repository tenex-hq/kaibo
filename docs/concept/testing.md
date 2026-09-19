# Why the testing rules are what they are

`AGENTS.md` states the testing rules as one-liners. This is the argument behind
them, for when one looks arbitrary and you are about to work around it.

## Count is not evidence

The suite exists to fail when the code is broken, and it has not always done
that: a mutation deleting the entire fencing mechanism left 80 tests green. A
test count says how many assertions ran, not how much of the product they hold.
Never cite one as evidence that something is covered.

## An expected value never comes from the code under test

Write the literal. A test that computes its expectation by calling the function
it is testing asserts self-consistency and nothing else - it stays green through
any change that is internally consistent, including a wrong one.

If writing the literal is painful, that pain is the test telling you the output
has no stable contract. Fix the contract rather than the test.

## Every test must be able to fail for the reason its name gives

If the name says "draft pages are excluded", breaking draft exclusion must turn
it red, and nothing else about the fixture may satisfy it by accident. A fixture
path containing the word "draft" is not a draft label, and a test that passes
because of the path name will keep passing after the label logic is deleted.

Check this by breaking the code, not by reading the test.

## A test describes a situation, not a function call

"Frontmatter with a malformed date field around a draft status" is a situation:
it survives the refactor that renames the function. "`read_frontmatter_facts`
returns `None`" is an implementation detail, and it has to be rewritten every
time the implementation moves, which is how a suite ends up describing code
nobody has read in a year.

## Test-first, with red-phase evidence

Write the test, run it, and keep the actual failure output. A test written after
the code has never been observed to fail, so nothing establishes that it can.
A PR claiming TDD without quoted failure output has not shown its work.

## Tests are hermetic by default

No network, no real subprocess, no filesystem mutation outside `tempfile`.
Hand-written fakes over mocking crates; `Clock` and `Environment` are injected
so no test touches ambient state. A suite that reads the wall clock or the real
environment fails on the machine that has a different one, and the failure is
attributed to the change rather than to the suite.

The one exception is opt-in behind an environment variable that is inert unless
set, so it never runs in CI: see
[`qmd_contract_check.rs`](../../crates/kaibo-core/src/status/qmd_contract_check.rs).

## Guardrail tests sweep, they do not enumerate

A test that lists three functions by name asserts a property of that list, not
of the codebase. The fourth function added later escapes it silently, and the
guardrail reports success while the hole it was written to close is open. Assert
over the pipeline, or scan the source.

## Mutation testing is a CI gate, not a command anyone runs

CI mutates the lines a PR changes and blocks the merge on a survivor. That is
the whole of it: no scheduled sweep, no author-time step. Running `cargo
mutants` locally mutates the whole tree, takes far longer than the gate, and
reports survivors on lines the PR never touched.

## The eval suites are not the test suite

`evals/` grades the shipped skills against a live model, at roughly a million
tokens for the cheap suite and markedly more for the graded one. `caliper
validate` is free and proves the spec still resolves. `caliper run` costs real
money, so it is a deliberate act and never a reflex. See
[`evals/README.md`](../../evals/README.md).
