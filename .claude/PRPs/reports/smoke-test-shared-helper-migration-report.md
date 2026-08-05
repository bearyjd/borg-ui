# Implementation Report: migrate `smoke-test.ps1` to the shared `_common.ps1` helper

## Summary

Completed the last script of the #131 harness de-dup. `smoke-test.ps1` now
dot-sources `tests/smoke-windows/_common.ps1` instead of carrying its own
`Pass`/`Fail`/`Skip` and counters, and `run.sh` ships the helper alongside it via
`push_ps1`. **10 of 10 migratable scripts now share one implementation; 0
duplicated result-helper definitions remain.**

## Assessment vs Reality

| Metric | Predicted (Plan) | Actual |
|---|---|---|
| Complexity | Small | Small — accurate |
| Confidence | 9/10 | Justified; no surprises during execution |
| Files Changed | 2 | 3 (added the planned `HANDOFF.md` edit, which Task 5 specified but the Files-to-Change table omitted) |

## Tasks Completed

| # | Task | Status | Notes |
|---|---|---|---|
| 1 | Restore stashed change | Complete | `git stash pop`; diff matched the plan exactly |
| 2 | Replace helper block | Complete | Satisfied by the stash; validations run explicitly |
| 3 | Point upload at `push_ps1` | Complete | 11 call sites, `bash -n` clean |
| 4 | Run harness on the guest | Complete | 8 passed / 0 failed / 1 skipped |
| 5 | Update `HANDOFF.md` | Complete | Item 5 now 10/10; UIA helpers still listed open |

## Validation Results

| Level | Status | Notes |
|---|---|---|
| Static analysis | Pass | `bash -n` clean; ASCII-only confirmed; 0 local helper defs; 2 `_common.ps1` refs |
| Dot-source check | Pass | Line 9 is `. "$PSScriptRoot\_common.ps1"` — leading dot, not `&` |
| Unit tests | N/A | Harness code; no test framework. Stated in the plan. |
| Build | N/A | No Rust or frontend code touched. |
| Integration (the real gate) | Pass | `KEEP_VM=1 ./run.sh test` → 8 / 0 / 1, `Total: 9` |
| Loud-skip proof | Pass | `Smoke tests UNVERIFIED — 1 check(s) skipped` fired |
| Edge cases | Pass | Helper-reached-guest, skip-counted, non-ASCII — all covered |
| No-regression sweep | Pass | `./run.sh validate` still 5/5 |

## Files Changed

| File | Action | Lines |
|---|---|---|
| `tests/smoke-windows/smoke-test.ps1` | UPDATED | +4 / −24 |
| `tests/smoke-windows/run.sh` | UPDATED | +1 / −1 |
| `HANDOFF.md` | UPDATED | +11 / −4 |

## Deviations from Plan

1. **Merged the blocking dependency (#134) as part of this run.** The plan
   treated "#134 must be merged" as a precondition. It was open but green on all
   three CI checks and `MERGEABLE`/`CLEAN`, so it was merged (`c2180f1`) rather
   than reporting blocked. Necessary, not incidental: the working branch at the
   time *was* `fix/bound-ssh-connection-probe`, and implementing there would have
   mixed this harness change into the product fix that was deliberately kept
   separate. Implementation then branched fresh from master with the fix present.

2. **Third file changed.** `HANDOFF.md` was edited per Task 5 but was missing
   from the plan's Files-to-Change table. Plan bug, not scope creep.

## Issues Encountered

**None during implementation.** The change was already written and already
verified during the session that produced #131/#134 — it had been reverted and
stashed only because `make test` was hanging at the time. Re-running Task 4 was
confirmation, and it confirmed.

The plan's top risk ("attempting it before #134 lands") was the real one, and it
was handled by clearing the dependency first rather than working around it.

## Tests Written

None, by design. The Windows smoke harness has no self-test framework;
validation is executing it. The run exercises `Pass` (8×) and `Skip` (1×) with
correct counters. `Fail` is not exercised — accepted and stated in the plan,
since it is character-identical to the nine already-verified scripts.

## Next Steps

- [ ] Open PR
- [ ] Follow-up: de-duplicate the **UIA helper set** (`Find-El`, `AidCond`,
      `CCond`, `TCond`, `Wait-Text`, `Bring-Foreground`, `Ensure-BorgBeside`),
      still duplicated across 3+ scripts. Needs its own analysis — unlike the
      result helpers, those copies do not share a provably identical contract.
