# Plan: migrate `smoke-test.ps1` to the shared `_common.ps1` helper

## Summary

Finish the last script of the #131 harness de-dup. `smoke-test.ps1` is the only
remaining script that still defines its own `Pass`/`Fail`/`Skip` and counters
instead of dot-sourcing `tests/smoke-windows/_common.ps1`. The change is two
files, already written, already verified on the KVM guest, and currently sitting
in `git stash@{0}`.

## User Story

As a maintainer of the Windows smoke harness, I want every validate script to
share one result-helper implementation, so that a fix to `Pass`/`Fail`/`Skip`
(such as the skip-counting bug fixed in #131) lands everywhere at once instead
of in 1 of 12 copies.

## Problem → Solution

9 of 10 migratable scripts share `_common.ps1`; `smoke-test.ps1` still carries a
17-line private copy → all 10 share one implementation, 0 duplicated definitions.

## Metadata

- **Complexity**: **Small** — 2 files, ~25 lines net removed
- **Source PRD**: N/A
- **PRD Phase**: N/A — completes queued harness item 5 (see `HANDOFF.md`)
- **Estimated Files**: 2

> **Honest scoping note.** This is a small, already-verified change. The plan is
> short on purpose; padding it would be noise. The only genuinely non-obvious
> parts are the **dependency on #134** and the **dot-source scoping rule**, both
> called out below.

---

## Blocking dependency — read first

**PR #134 (`fix(ssh): bound the connection probe`) must be merged before this
plan can be validated.**

Before #134, `ssh::test_connection` had no timeout and Windows OpenSSH ignores
`-o ConnectTimeout`, so `ssh::tests::test_connection_errors_for_closed_port`
hung forever → `cargo test -p borg-core` hung → **`run.sh test` hung**. Since
`run.sh test` is the *only* way to exercise `smoke-test.ps1`, this change was
unverifiable and was deliberately held back rather than committed blind.

If `make test` hangs when you run the validation below, check that #134 is on
`master` and that the guest's `C:\borgui-test` has the patched `ssh.rs`.

---

## UX Design

**N/A — internal harness change.** No product code, no user-facing surface.
Console output is byte-identical: `_common.ps1`'s helpers were derived from this
exact variant (multi-line, `$name, $detail`, same colors), so there is not even
the color delta that three other migrated scripts had.

---

## Mandatory Reading

| Priority | File | Lines | Why |
|---|---|---|---|
| P0 | `tests/smoke-windows/_common.ps1` | all (46) | The helper being adopted; its header documents the dot-source rule and the output contract |
| P0 | `tests/smoke-windows/smoke-test.ps1` | 1-40, 195-210 | The block to delete, and the summary block that must keep working |
| P0 | `tests/smoke-windows/run.sh` | 51-62, 182-195 | `push_ps1` definition and the one upload line to change |
| P1 | `tests/smoke-windows/validate.ps1` | 1-25 | A completed migration of the *identical* variant — copy this shape |
| P2 | `HANDOFF.md` | "Harness gaps and queued work" | Item 5 status; update after merge |

## External Documentation

**None needed.** No new dependencies, no external APIs. The one PowerShell
subtlety (dot-source scoping) is documented inline in `_common.ps1`.

---

## Patterns to Mirror

### DOT_SOURCE_ADOPTION
```powershell
# SOURCE: tests/smoke-windows/validate.ps1:18-23 (already migrated, same variant)
$ErrorActionPreference = "Continue"

# Counters + Pass/Fail/Skip. Dot-sourced so they run in this script's scope;
# run.sh's push_ps1 uploads _common.ps1 alongside this file.
. "$PSScriptRoot\_common.ps1"
```

### SHARED_HELPER_CONTRACT
```powershell
# SOURCE: tests/smoke-windows/_common.ps1:19-45
$script:Passed = 0
$script:Failed = 0
$script:Skipped = 0
$script:Results = @()

function Pass($name, $detail) {
    $script:Passed++; $script:Results += @{ Name = $name; Status = "PASS"; Detail = $detail }
    Write-Host "  PASS: $name" -ForegroundColor Green
    if ($detail) { Write-Host "        $detail" -ForegroundColor DarkGray }
}
```

### UPLOAD_PATTERN
```bash
# SOURCE: tests/smoke-windows/run.sh:58-62
push_ps1() {
    local name="$1" user="${2:-$SSH_USER}"
    $SCP_CMD "$SCRIPT_DIR/_common.ps1" "$user@$SSH_HOST:_common.ps1"
    $SCP_CMD "$SCRIPT_DIR/$name" "$user@$SSH_HOST:$name"
}
```

### CALL_SITE
```bash
# SOURCE: tests/smoke-windows/run.sh:209 (run_validate, already migrated)
    push_ps1 validate.ps1
```

---

## Files to Change

| File | Action | Justification |
|---|---|---|
| `tests/smoke-windows/smoke-test.ps1` | UPDATE | Delete the 4 counter lines + 3 helper definitions; dot-source `_common.ps1` |
| `tests/smoke-windows/run.sh` | UPDATE | One line: raw `$SCP_CMD` → `push_ps1 smoke-test.ps1` |

## NOT Building

- **No change to `validate-installer.ps1` / `validate-updater.ps1`** — neither
  keeps a `$script:Results` array, and `updater` has its own `Finish` writing
  `updater-smoke-result.json`. Migrating them would change what they emit.
- **No change to `validate-autostart-login.ps1`** — uses a `Res()` helper, not
  `Pass`/`Fail`/`Skip`.
- **No de-duplication of the UIA helper set** (`Find-El`, `AidCond`, `CCond`,
  `TCond`, `Wait-Text`, `Bring-Foreground`, `Ensure-BorgBeside`). Still
  duplicated across 3+ scripts; separate work, and those copies do **not** share
  a provably identical contract the way the result helpers do.
- **No change to `smoke-test.ps1`'s summary block or its `smoke-results.json`
  output.** The counters it reads keep the same names.
- **No new tests.** The harness has no self-tests; validation is running it.

---

## Step-by-Step Tasks

### Task 1: Restore the stashed change (fast path)

- **ACTION**: `git stash pop` — the exact change is already staged in `stash@{0}`.
- **IMPLEMENT**: Nothing to write; verify the result matches Tasks 2-3 below.
- **VALIDATE**:
  ```bash
  git diff --stat   # expect: run.sh (1 line), smoke-test.ps1 (~25 removed)
  grep -c '^function \(Pass\|Fail\|Skip\)' tests/smoke-windows/smoke-test.ps1  # expect 0
  grep -c '_common.ps1' tests/smoke-windows/smoke-test.ps1                     # expect 2
  ```
- **GOTCHA**: If the stash is gone (dropped, or a fresh clone), skip to Task 2
  and apply by hand — it is a 2-minute edit.

### Task 2: Replace the helper block in `smoke-test.ps1`

- **ACTION**: Delete lines 6-9 (the four `$script:*` initialisers) and the three
  `function Pass/Fail/Skip` definitions; insert the dot-source.
- **IMPLEMENT**: After `$ErrorActionPreference = "Continue"`, put:
  ```powershell
  # Counters + Pass/Fail/Skip. Dot-sourced so they run in this script's scope;
  # run.sh's push_ps1 uploads _common.ps1 alongside this file.
  . "$PSScriptRoot\_common.ps1"
  ```
  Leave `Write-TestHeader` in place — it is script-specific (`--- TEST: ---`).
- **MIRROR**: `DOT_SOURCE_ADOPTION`.
- **GOTCHA 1 — dot-source, never `&`.** Dot-sourcing runs the file in the
  caller's scope so `$script:Passed` binds to this script. `& "$PSScriptRoot\_common.ps1"`
  gives the helpers their own scope and **every counter silently stays 0** —
  the suite would report `Passed: 0` and `run.sh`'s `grep "Failed: 0"` would
  still pass, so this fails *silently*.
- **GOTCHA 2 — ASCII only.** PowerShell 5.1 reads a UTF-8 file without a BOM as
  ANSI; one non-ASCII byte breaks parsing.
- **VALIDATE**:
  ```bash
  LC_ALL=C grep -n '[^ -~\t]' tests/smoke-windows/smoke-test.ps1 && echo NON-ASCII || echo ok
  ```

### Task 3: Point the upload site at `push_ps1`

- **ACTION**: In `run.sh`'s `run_tests()` (line ~185) replace the raw SCP with
  `push_ps1 smoke-test.ps1`.
- **IMPLEMENT**:
  ```bash
  -    $SCP_CMD "$SCRIPT_DIR/smoke-test.ps1" "$SSH_USER@$SSH_HOST:smoke-test.ps1"
  +    push_ps1 smoke-test.ps1
  ```
- **MIRROR**: `CALL_SITE`.
- **GOTCHA**: Without this, `_common.ps1` never reaches the guest and the script
  dies with `Cannot find path ...\_common.ps1`. `smoke-test.ps1` runs as the
  default `$SSH_USER`, so no second user argument is needed (unlike
  `validate-edge.ps1`).
- **VALIDATE**:
  ```bash
  bash -n tests/smoke-windows/run.sh
  grep -c 'push_ps1 ' tests/smoke-windows/run.sh   # expect 11
  ```

### Task 4: Run it on the guest

- **ACTION**: `cd tests/smoke-windows && KEEP_VM=1 ./run.sh test`
- **GOTCHA — always prefix `KEEP_VM=1`.** `run.sh` has `trap cleanup EXIT` and
  tears the container down without it, costing a full VM boot.
- **VALIDATE**: Expect `Passed: 8  Failed: 0  Skipped: 1`, plus the loud-skip
  line `Smoke tests UNVERIFIED — 1 check(s) skipped`. The skip is
  `e2e_backup_restore`, which correctly skips without `BORG_TEST_BIN`.
  Seeing that warning is itself proof the shared `Skip` incremented the counter.

### Task 5: Update `HANDOFF.md`

- **ACTION**: In "Harness gaps and queued work", move `smoke-test.ps1` from
  still-open to done, so item 5 reads 10 of 10 for the result helpers.
- **GOTCHA**: Leave the **UIA helper set** listed as still open — it is.

---

## Testing Strategy

No unit tests: this is shell/PowerShell harness code with no test framework.
Validation is executing the harness, which exercises all three helpers.

| Check | Input | Expected | Edge case? |
|---|---|---|---|
| `Pass` counts | 8 passing checks | `Passed: 8` | no |
| `Skip` counts | e2e without `BORG_TEST_BIN` | `Skipped: 1` + UNVERIFIED warning | **yes** — the #131 bug class |
| `Fail` counts | not triggered | n/a | untested by design; identical code to 9 verified scripts |
| Upload | `push_ps1` | no `Cannot find path` | yes |

### Edge Cases Checklist
- [x] Helper file missing on guest — covered by Task 3's validate
- [x] Skip not counted — covered by the UNVERIFIED warning in Task 4
- [x] Non-ASCII bytes — covered by Task 2's validate
- [ ] `Fail` path — not exercised; accepted, code is identical to 9 verified scripts

---

## Validation Commands

### Static
```bash
bash -n tests/smoke-windows/run.sh
LC_ALL=C grep -n '[^ -~\t]' tests/smoke-windows/smoke-test.ps1 || echo "ascii ok"
grep -c '^function \(Pass\|Fail\|Skip\)' tests/smoke-windows/smoke-test.ps1   # expect 0
```
EXPECT: syntax OK, ascii ok, 0 local helper definitions.

### Harness run (the real gate)
```bash
cd tests/smoke-windows && KEEP_VM=1 ./run.sh test
```
EXPECT: `Passed: 8  Failed: 0  Skipped: 1` and the UNVERIFIED warning.

### No-regression sweep (optional, ~10 min)
```bash
cd tests/smoke-windows
KEEP_VM=1 ./run.sh validate        # expect 5/5
KEEP_VM=1 ./run.sh validate-vss    # expect 4/4
```
EXPECT: unchanged. These already use `_common.ps1`; run them only if `run.sh`
was touched beyond the single line.

### CI
EXPECT: Frontend / Rust / Rust (Windows) all pass — and note they prove
**nothing** here. CI never executes `tests/smoke-windows/`. The harness run
above is the only real evidence.

### Manual
- [ ] `_common.ps1` reached the guest (no `Cannot find path`)
- [ ] Counters non-zero (a `& `-instead-of-dot-source bug shows as `Passed: 0`)

---

## Acceptance Criteria
- [ ] `smoke-test.ps1` has 0 local `Pass`/`Fail`/`Skip` definitions
- [ ] It dot-sources `_common.ps1`
- [ ] `run.sh` uses `push_ps1 smoke-test.ps1` (11 call sites total)
- [ ] `KEEP_VM=1 ./run.sh test` → 8 passed / 0 failed / 1 skipped
- [ ] The UNVERIFIED loud-skip warning appears
- [ ] `HANDOFF.md` item 5 updated

## Completion Checklist
- [ ] Matches the shape of the 9 already-migrated scripts
- [ ] ASCII-only
- [ ] Console output unchanged
- [ ] `smoke-results.json` unchanged
- [ ] No scope creep into installer/updater/autostart or the UIA helpers

## Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| #134 not merged → `make test` hangs, looks like this change broke it | Medium | High — misattributed | Confirm #134 on `master` first; a hang here is the ssh bug, not this |
| `&` used instead of dot-source | Low | High — **silent**, counters stay 0 | Assert non-zero `Passed:` in Task 4 |
| `push_ps1` line forgotten | Low | Low — loud `Cannot find path` | Task 3 validate |
| Stash dropped before restore | Low | Low | Tasks 2-3 fully specify the edit by hand |

## Notes

**This change is already verified.** It was applied and run on the guest during
the session that produced #131/#134: `8 passed, 0 failed, 1 skipped`, with the
loud-skip warning firing. It was reverted and stashed **only** because at that
moment `make test` was hanging (the #134 ssh bug) and committing an unverified
script would have broken the standard the other nine were held to. Re-running
Task 4 is confirmation, not discovery.

**Confidence: 9/10** for single-pass implementation. The single realistic
failure mode is attempting it before #134 lands.
