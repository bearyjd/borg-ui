# BorgUI handoff

Last updated: 2026-08-06. `master` includes **#137**, version **0.3.1**, two
open issues (#64, #114).

**#118–#137 are on `master` but not in any release.** `v0.3.1` tags #117, and
`master` is 19 commits past it — verify with
`git log --oneline v0.3.1..master` rather than trusting this range, which goes
stale on every merge. Most of it is smoke-harness and documentation work; two
entries are user-affecting:

- **#128** restructured the Tauri command layer (see "Architecture map") — a
  refactor with no user-visible change beyond one error message. Any note citing
  `app-tauri/src-tauri/src/commands.rs` predates it and names a file that no
  longer exists.
- **#134** bounds the SSH connection probe, so a filtered host can no longer
  hang the UI.

**#128 is runtime-verified on Windows, not just CI-green (2026-08-05).** The
whole point of the KVM harness is that `Rust (Windows)` CI compiles and
unit-tests but stages a placeholder `borg.exe` and never runs a backup. The full
matrix was run against a `borg-ui.exe` built from post-#128 `master` on the
guest, with **zero failures and zero unexplained skips**:

| Check | Result |
|---|---|
| `validate` (engine, autostart reg, schtasks, local drive repo) | 5/5 |
| `validate-vss` (scheduled path, exclusively-locked file) | 4/4 |
| `validate-vss-manual` (GUI path) | 3/3 |
| `validate-gui` (keychain + real scheduled task fires) | 2/2 gating |
| `validate-gui-flows` (nav, profile switch, restore, cancel) | 4/4 |
| `validate-tray` | 3/3 |
| `validate-archive-smoke` (100k entries) | 5/5 |
| `validate-edge` (non-admin) | 2/2 |
| `validate-installer` (published v0.3.1 NSIS + MSI) | 12/12 |

That covers the VSS wiring #128 moved into `commands/backup.rs`: the
exclusively-locked file landed in the archive, which a live-file fallback could
not have done. `validate-installer` ran against the **real published v0.3.1
artifacts**, including the `nsis_render`/`msi_render` launch checks.

Staging trick used for the UIA scripts (they hardcode
`C:\borgui-test\target\release\borg-ui.exe` and need a real rendered WebView, which
a plain `cargo build --release` does not produce): silent-install the v0.3.1 NSIS,
then copy `%LOCALAPPDATA%\BorgUI\*` to that path. Always prefix `KEEP_VM=1` or
`run.sh` tears the container down on EXIT.

> This file is a *living status* file, not a changelog. Everything above the
> "History" heading describes the present. If a claim here disagrees with the
> code, the code wins — run `git log --oneline -- <the cited path>` and fix this
> file. It has gone stale before (see "Trust rules" below).

## Start here: 0.3.1 is published and updater-verified

`v0.3.1` is published on GitHub (2026-08-03), so `releases/latest` resolves to
it and the in-app updater can offer it to eligible installed clients.

The complete public release has MSI + NSIS + both `.sig` files + `latest.json`
(version `0.3.1`, correct URL, 416-byte signature).

The installed-app updater has also passed on the Windows KVM guest: public
v0.3.0 NSIS baseline → update prompt → user confirmation → installed v0.3.1
(3 passed, 0 failed, 0 skipped, 2026-08-03).

`validate-updater.ps1` takes `-BaselineInstaller` and `-ExpectedVersion` as
parameters and hardcodes no version, so retain it for future release checks.

```bash
# Verified on 2026-08-03:
make -C tests/smoke-windows validate-updater \
  BASELINE_INSTALLER=<v0.3.0 -setup.exe> EXPECTED_UPDATE_VERSION=0.3.1
```

### Release state

| Tag | GitHub release | Notes |
|---|---|---|
| `v0.3.1` | **Latest** (published 2026-08-03) | public release; updater path passed from v0.3.0 |
| `v0.3.0` | published (2026-07-05) | prior release and updater-test baseline |
| `v0.3.0` | Draft (2026-07-05) | stale duplicate sitting beside the published one; harmless |
| `v0.2.0`, `v0.1.0` | published | — |

Installers are intentionally **unsigned** (Authenticode is #64). Updater signing
is separate from Authenticode; the updater private key lives only in GitHub
Actions secrets.

## Open issues, with the next concrete step for each

- **#64 — production Authenticode signing.** Blocked on Azure provisioning only.
  The repo side is complete and the ordering is correct (Authenticode signs,
  *then* updater `.sig` files are regenerated, *then* `latest.json` is built). A
  full runbook and the exact missing secrets/variables are in a comment on the
  issue. No repository variables are currently set, so `vars.AZURE_*` resolve to
  empty. Must not weaken or block unsigned development dry runs.
- **#114 — archive browser under-counts entries** (99799 of 100000). #116 fixes
  the race it is attributed to and makes an incomplete listing *visible* instead
  of silent. Reproduces ~1 run in 6, so the fix is justified by code reading and
  arithmetic, **not demonstrated** — three green post-fix runs would occur ~58%
  of the time even on unfixed code. Left open deliberately. Close it when the
  warning has had field exposure, or when someone reproduces an under-count
  against a build containing #116.

  **Evidence gathered 2026-08-05 (posted to the issue): 17 consecutive clean
  runs.** `validate-archive-smoke` x17 on the KVM guest against a production
  v0.3.1 build (contains #116). The count assertion passed **17/17** —
  `header shows 100000 / 100000` every time, zero `incomplete` banners.
  **(5/6)^17 ≈ 4.5%**, so this inverts the original arithmetic instead of
  repeating it.

  What makes those runs count: each logged a **progressive count mid-stream
  (85000-95000)** before the tree was built, confirming every run entered the
  race window #116 closed. A run finishing too fast to show a partial count
  would not have exercised the bug.

  **Still not closed, deliberately.** 17 back-to-back runs on one machine, one
  archive shape, one fixture generator is *controlled repetition*, not the
  "field exposure across varied conditions" this criterion asks for. Strong
  evidence the race is fixed; not proof it cannot reproduce elsewhere. The
  disposition call is the maintainer's.
## Windows verification: what is actually proven

**39 smoke checks reported green (2026-07-31/08-01)** against a production build
on the KVM guest: engine, non-admin, VSS† (including backing up an
exclusively-locked file), scheduled task, tray, GUI flows (nav / profile switch /
restore round-trip / cancel), NSIS + MSI install+uninstall.

**† VSS is verified independently (2026-08-02): 4 passed, 0 failed, 0
skipped.** The corrected validator drove `--scheduled-backup` from the Windows
guest, confirmed the exclusively locked file was archived (proving a snapshot,
not live-file fallback), checked clean stored paths, and restored both files
byte-correct. The old reported VSS result remains unreconciled, but this fresh
run replaces it as the evidence.

**Credential Manager is verified on Windows (2026-08-03).** The session-1
`borg-platform-win` test passed set → fresh get → `cmdkey` visibility → clear,
without linking Tauri/WebView2. The tracking issue #119 is closed.

Note this is a claim about *one reported run*, not about coverage being new.
An earlier revision of this file said "all of these were compile-only before
this pass" — that is false: `validate-edge.ps1` (#32), `validate-tray.ps1` (#34),
`validate-gui-flows.ps1` (#37), `validate-vss.ps1` (#40) and
`validate-installer.ps1` (#45) all landed in June/early July and had been run
before. What is new is the consolidated 39-check run against a production build.

What CI does **not** prove (also in `CLAUDE.md`):

- The Linux `rust` job only compiles `cfg(not(windows))` code — VSS, scheduler,
  autostart and cloud placeholders are invisible to it.
- The `rust-windows` job compiles, clippies (`--all-targets`) and unit-tests the
  Windows code, but stages a **placeholder empty `borg.exe`** and never runs a
  real backup, VSS snapshot or scheduled task.
- `crates/borg-core/tests/e2e_backup_restore.rs` silently **skips** (not fails)
  unless `BORG_TEST_BIN` is set.

### Still unrun / uncovered

- `validate-autostart-login` — previously passed once on the KVM guest (see the
  harness README); repeat it against the public v0.3.1 binary when the guest is
  available. It reboots the guest.
- Multi-drive edge checks — SKIP without a D: drive; `make edge-all` recreates
  the VM with one.
- New #71–#80 features (wake / battery / Wi-Fi / removable / USB, SSH and
  secondary destinations, OneDrive placeholders, reporting delivery, recovery on
  a clean VM, all five templates through backup/restore) have **no smoke
  automation**.

## Harness gaps and queued work

**`make test` stalled on the KVM guest — ROOT-CAUSED AND FIXED (2026-08-05).**
It was never a harness problem. **`ssh::test_connection` had no timeout of its
own**, and Windows OpenSSH does not honour `-o ConnectTimeout`: measured
directly on the guest, `ssh.exe -o BatchMode=yes -o ConnectTimeout=10 -p 61234
nobody@127.0.0.1` was **still running after 90s**. That hung
`ssh::tests::test_connection_errors_for_closed_port`, which hung
`cargo test -p borg-core`, which hung `run.sh test`.

**The real bug was user-facing, not test-only.** `test_ssh_connection` — the
"Test connection" button — awaits `test_connection` directly with no timeout, so
on Windows a host whose port is filtered would hang the UI indefinitely, with no
timeout and no cancel. Every borg spawn in `borg.rs` is wrapped in
`tokio::time::timeout`, and `check_host_reachable` bounds its TCP connect at 5s;
this one spawn was the gap. Fixed by bounding it at
`CONNECT_TEST_TIMEOUT_SECS = 30` plus `kill_on_drop(true)` so a hung `ssh.exe`
is reaped instead of accumulating. Verified: borg-core is **158 passed / 0
failed on Windows** (was an infinite hang) and 173/0 on Linux; `make test` now
completes 8 passed / 0 failed / 1 skipped.

**Two wrong diagnoses were published before the right one — recorded so nobody
re-runs them.** An earlier revision of this entry blamed a **cargo package-cache
lock deadlock**; the ~6-process `cargo` tree that suggested it was a *symptom*
(orphaned children of the hung test), not the cause. The second guess was the
PyInstaller e2e spawn-hang documented under "Trust rules" — also wrong,
`e2e_backup_restore` completes fine. What settled it: running cargo directly
instead of through `run.sh`'s output-capturing `$(...)`, where
`cargo test -p borg-core --no-run` **finished in 8.4s**, proving compilation was
never involved and the hang was in *running* the tests. Bisecting the test
binaries then named the culprit in one pass.

**Side effect worth knowing:** those three `test_connection` tests now take ~30s
on Windows, because they genuinely wait out the new bound. Correct, but slower —
not a regression.

Still true and unrelated: `run_tests()` in `run.sh` sets no environment while
`build_app()` sets `PATH`/`CARGO_NET_OFFLINE`. Harmless today because
`smoke-test.ps1` sets its own, but the asymmetry is a trap.

**`validate-installer.ps1` now launches the installed `borg-ui.exe` in the
interactive desktop.** It requires an accessible rendered WebView window and
rejects the known Vite localhost error page, before it exercises the installed
`borg.exe` round-trip. This closes the harness hole that let #85 (missing CRT)
and #86 (installed app loading the Vite dev server) evade the old layout-only
check. It passed against both public v0.3.1 release installers on the Windows
KVM guest (NSIS + MSI: 12 passed, 0 failed, 0 skipped; 2026-08-03), including
the interactive render probe, Borg layout, engine round-trip, and uninstall.

**Queued harness plan, items 3–5 of 5** (1–2 shipped in #120):

3. Outcome-based assertions instead of internal shapes.
4. **Loud skips — shipped.** A permanently-skipping check now emits an
   `UNVERIFIED` warning with its skipped count across the desktop and legacy
   harness entry points. A skip remains non-fatal where the host legitimately
   lacks the prerequisite, but it can no longer be mistaken for coverage.
5. De-duplicate the copy-pasted blocks. **Result helpers: DONE in #131
   (2026-08-05).** `Pass`/`Fail`/`Skip` plus their counters now live in
   `tests/smoke-windows/_common.ps1`, dot-sourced by 9 scripts — 17 duplicated
   definitions removed. Each migrated script was **run on the guest and compared
   to its pre-change baseline** (see the table above); a green build proves
   nothing for harness code.
   - `push_ps1 <script> [user]` in `run.sh` uploads the helper beside each
     script, because every script is scp'd standalone and run with
     `powershell -File` — there is no shared upload path. It takes a user because
     `validate-edge` runs as the admin *and* `borgstd`.
   - Dot-source, never `&`: dot-sourcing runs in the caller's scope so
     `$script:Passed` binds correctly; `&` would leave every counter at 0.
   - **Left out on purpose:** `validate-installer` / `validate-updater` (neither
     keeps a `$script:Results` array; updater has its own `Finish` writing
     `updater-smoke-result.json`) and `validate-autostart-login` (uses `Res()`).
     Sharing would change what they emit.
   - `smoke-test.ps1` was held back from #131 because `make test` was hanging
     (the ssh bug fixed in #134) and it could not be verified. **Landed once that
     unblocked it: 10 of 10 migratable scripts now share `_common.ps1`**, checked
     by `KEEP_VM=1 ./run.sh test` → 8 passed / 0 failed / 1 skipped, with the
     loud-skip warning firing (the `e2e_backup_restore` skip is expected without
     `BORG_TEST_BIN`, and seeing it counted proves the shared `Skip` works).
   - **The remaining "UIA helper set" item is mostly a mirage — measured
     2026-08-05, recommend closing it.** Of **28 helpers defined in more than one
     script, only 5 are byte-identical**; the other **23 have drifted into
     genuinely different implementations**. Worst cases: `Invoke-Borg` has **6**
     variants, `Ensure-BorgBeside` **5**, `Write-TestHeader` **8**.

     Crucially, much of that drift is **intentional, not rot**:
     `Write-TestHeader`, `Hdr` and `Summary` each print their own section label
     (`--- TEST:`, `--- VALIDATE-VSS:`, `--- GUI-FLOW:` …), so collapsing them
     would be a regression. `Invoke-Borg`'s variants differ in timeout and cwd
     handling per call site.

     What is actually shareable: `AidCond`, `CCond`, `TCond` (x3 each),
     `Wait-Text` (x2), `Signal` (x2) — about 11 definitions, three of which are
     one-line UIA condition constructors. Extracting those would add a shared-file
     dependency to three more scripts to remove eleven trivial lines: worse than
     the duplication. **Do not lift-and-shift this set.** If someone touches it,
     re-measure first — the count in any older note is wrong.

   **Bug fixed along the way:** `validate-vss-spike.ps1` calls `Skip` 9 times but
   its local `Skip` never incremented a counter, never declared `$script:Skipped`,
   and its summary printed no `Skipped:` line — so `report_skips` could not see
   it. That is exactly the "a permanently-skipping check looks identical to a
   passing one" hole item 4 closed elsewhere, still live in that script. It now
   counts and reports skips (verified on the guest).

**The stale `validate-vss.ps1` `history.json` gate is fixed and re-run on
Windows (2026-08-02).** The script polls `borg list --json` for the newly-created
archive, then uses that archive for its locked-file and restore assertions.
This outcome-based check passed 4/0/0; see the VSS note above.

**Staging a production exe for GUI smoke** without a 15–30 min VM build: install
the NSIS, then copy `%LOCALAPPDATA%\BorgUI` to `C:\borgui-test\target\release\`
(the path the GUI scripts hardcode); drive it via `/IT` session-1 `schtasks`.
GUI rendering is documented as a manual VNC check but *is* scriptable that way —
SSH sits in session 0 and always reports `MainWindowHandle=0`.

## Trust rules — read before acting on anything above

**Treat smoke output as a hypothesis and check it against the code.** Of five
smoke failures investigated on 2026-08-01, **four were tests or fixtures, not
the app**, and two app-bug conclusions had to be retracted:

- a UTF-8 BOM in a hand-written `profiles.json` fixture (serde rejects it; the
  app correctly found no profile),
- a `history.json` assertion left stale by the move to SQLite,
- a restore assertion polling one directory level too shallow, because the app
  nests restores under `BorgUI Restore <timestamp>\`,
- …and that same shallow assertion was *masking* the one genuine bug (#116).

**A sixth instance, 2026-08-05:** `validate-archive-smoke`'s
`browser_selective_restore` failed once in 18 runs with
`restore destination dialog: dialog still open after Ctrl+L navigate + select`.
That is the **native folder picker failing to dismiss under UI automation** —
the restore never started, so the failure carries no information about restore
correctness. The count assertion passed on that same run. A clean re-run passed
5/5, confirming a transient UIA flake (HANDOFF already records an earlier
selective-restore timeout). **Read the detail line before believing the check
name:** "selective restore failed" sounds like data loss and was a stuck dialog.
One re-run distinguishes *sometimes* from *always*; it does not measure the
flake rate, so if this starts costing time it needs its own repetition study.

**Two fixes shipped broken and needed a second pass** — worth remembering when
reviewing this class of change. #106 was a **silent no-op**: a SQLite CHECK
constraint rejected the new readiness kind and the failure was swallowed by a
`warn!`. #107 made both `authenticated` encryption modes **impossible to
create**: the passphrase rule was duplicated in the frontend and only the
backend was updated. Neither was visible to unit tests over pure functions;
catching this class needs tests that cross a boundary — the database, or
Rust↔TypeScript parity.

**Every SHA older than 2026-07-31 is dead.** ~48 MB of installer binaries were
committed by mistake in #110 and purged with `git filter-repo`, so **every
commit SHA changed** (filter-repo strips GitHub's merge signatures, which
cascades) and the `v0.1.0`/`v0.2.0`/`v0.3.0` tags were repointed. Release
*assets* are untouched and the updater still works. Any clone predating
`fa2d955` must be re-cloned. Pre-rewrite SHAs cited in old notes — `3b9add5`,
`2b826c2`, `27680a1` and friends — no longer resolve; look up work by **PR
number**, not hash.

## Schema endpoints

- Profile schema: **v11** — `PROFILE_SCHEMA_VERSION`,
  `app-tauri/src-tauri/src/profiles.rs:7`.
- SQLite schema: **v9** — `DATABASE_SCHEMA_VERSION`,
  `app-tauri/src-tauri/src/history.rs:9`.
- Older profile data migrates to the current schema; future versions are
  rejected without overwriting them.
- `BackupSelection` carries `template_id` and `template_version`.

When changing either schema, preserve atomic migration behavior and
future-version rejection. Do not reuse old version numbers.

**Migration test coverage is asymmetric — verified 2026-08-01.** The profile
side has real regression tests: `older_profiles_are_migrated_to_current_schema`,
`unversioned_profiles_are_migrated_and_persisted`,
`v3_schedule_paths_migrate_to_canonical_selection`, and
`future_schema_is_rejected_without_overwrite`. The SQLite side has two:
`migrates_legacy_once_and_retains_source` (legacy `history.json` → SQLite
import) and `upgrading_an_old_database_accepts_rotation_events_and_keeps_rows`
(`history.rs:1035` — builds an old on-disk `readiness_events` table with the
pre-rotation CHECK constraint, upgrades it, and asserts the rows survive).

What is **missing** is a generic schema-version walk — nothing exercises an
on-disk v3 database up to the current v9. Earlier revisions of this file claimed
"SQLite schema 3 → 7 on-disk, data-preserving and idempotent", which named the
wrong version and overstated coverage; a correction pass then swung too far and
said the SQLite side had "only" the legacy-import test, which undersold it.
Re-check the version constants and test names above rather than trusting this
paragraph — it has now been wrong in both directions.

## Architecture map

- `crates/borg-core` — portable Borg CLI wrapper, cancellation, validation,
  progress parsing, archive operations, SSH helpers. Must stay
  platform-agnostic: no `cfg(windows)` here.
- `crates/borg-platform-win` — VSS, Task Scheduler, autostart, power/WLAN APIs,
  cloud-file detection/hydration, other Windows integrations.
- `app-tauri/src-tauri` — Tauri IPC, profiles, SQLite history, scheduling,
  reporting, recovery, health, forecasting, templates, backup orchestration.
  The IPC surface is `src/commands/` — **16 domain modules since #128**, not the
  single 2,781-line `commands.rs` that older notes cite. `commands/mod.rs`
  re-exports every command, so `commands::<name>` paths and the
  `generate_handler!` list in `lib.rs` are unchanged; it also holds what the
  domain modules share (`AppState`, the operation-registry keys, the
  profile/config helpers), which submodules reach via `use super::*`. Add a new
  command to the module matching its domain, not to `mod.rs`.
- `app-tauri/src` — Svelte 5 UI and stores.
- `tests/smoke-windows` — KVM/Windows installer, updater, GUI, VSS, scheduler
  and archive smoke harness.

## Security and privacy invariants

Keep these out of logs, diagnostics exports, history/config exports and report
payloads:

- repository and SMTP passphrases
- webhook secrets
- SSH private keys and recovery payloads
- source listings and archive filenames
- temporary restore paths

One deliberate exception, decided in #101: a borg *warning* naming the single
file it failed on does reach `borgui.log` and the support bundle, because that
is the diagnostic content the log exists for. Account names are scrubbed from
paths (`redaction.rs` rewrites `C:\Users\<name>` and `/home/<name>`) and the
Diagnostics section says plainly that a bundle can still name individual files.
The file-by-file `archive_progress` stream is never logged.

Repository metrics are aggregate-only. Restore-search results are streamed, not
persisted. Readiness events store typed outcomes and timestamps, not recovery
file locations or passphrases. **Credential Manager is the sole authority** for
repository passphrases, secondary credentials, webhook URLs and SMTP passwords —
never persist them to `profiles.rs` config or SQLite.

**The option-injection gate:** `borg` and `ssh` are spawned via direct argv, so
shell metacharacters are not the risk — a value beginning with `-` being parsed
as a flag is. Every argv-bound untrusted string must pass `reject_option_like`
(`crates/borg-core/src/config.rs:25-36`) before reaching a `Command`. If you add
any new user-controlled field that becomes an argv token, route it through that
gate and add a `..._rejects_option_like_...` test alongside the existing ones.

## Operational gotchas

- Keep Borg prompts disabled in GUI and headless paths.
- Keep raw Windows drive-letter repository behavior; do not restore the old
  administrative-share rewrite.
- Manual and scheduled backups must use the same `BackupSelection`.
- Placeholder scanning/materialization must finish before VSS creation.
- A multi-destination run must reuse one VSS snapshot and archive name.
- Cancellation stops the active destination and skips remaining destinations.
- Append-only client access must not run compact; prune/delete remain logical.
- Manual backups ignore battery/Wi-Fi skip policy but still honor bandwidth and
  sleep prevention.
- Templates resolve known folders when listed/applied and never silently update
  an existing explicit selection.
- Keep Windows PowerShell smoke scripts **ASCII-only** for PowerShell 5.1 (CI
  guards this since #120).
- `./run.sh <subcmd>` in the smoke harness tears the VM down on EXIT unless
  `KEEP_VM=1` is set.

## Verification gates

Every feature PR passes:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cd app-tauri && pnpm check && pnpm test && pnpm build
git diff --check
```

**What the frontend CI job actually runs** (`.github/workflows/ci.yml`, verified
2026-08-01) is `pnpm install --frozen-lockfile` → `pnpm exec svelte-kit sync` →
`pnpm check` → `pnpm test`. There is **no `pnpm build`** in that job — the
frontend is built by the release workflow, not by ordinary CI. Keep `pnpm build`
in the local gate above anyway: it is the step that catches a broken production
bundle before it reaches a release.

Smoke entry points (read `tests/smoke-windows/README.md` before provisioning;
some targets need a production Tauri exe, an installer directory, or
`BASELINE_INSTALLER`):

```bash
make -C tests/smoke-windows vm
make -C tests/smoke-windows ssh
make -C tests/smoke-windows validate           # borg / reg / schtasks
make -C tests/smoke-windows validate-vss
make -C tests/smoke-windows validate-vss-manual
make -C tests/smoke-windows validate-gui-flows
make -C tests/smoke-windows validate-installer
make -C tests/smoke-windows validate-updater
make -C tests/smoke-windows validate-all
```

---

# History

Chronological record. **Nothing below is a current-status claim**, and the SHAs
that once appeared here died in the 2026-07-31 history rewrite — work is
identified by PR number.

## 0.3.1 cycle (2026-07-30 → 08-01)

Raised by the security and adversarial review passes over #99, plus harness work:

- **#99** — passphrase rotation. "Change passphrase" only overwrote the
  Credential Manager copy, silently desyncing it from the repository. The flow
  now runs `borg key change-passphrase` first and writes the keychain only after
  the repo accepts. Both partial-failure states are reported honestly, including
  a timeout whose outcome is genuinely unknown (borg is deliberately *not*
  killed — killing it mid-key-write risks destroying the key). Verified against
  real borg 1.4.4 for `repokey` and `keyfile`.
- **#106, #110, #113** — recovery readiness follows rotations; an exported key
  carries the passphrase current at export time. #113 also reports that
  importing a recovery key can revert the passphrase, and treats an import as
  proof-of-key.
- **#107, #110** — a stored passphrase is probed with `borg info` and rejected
  only on a definite wrong-passphrase verdict; `authenticated` repos are no
  longer created with an empty passphrase.
- **#108, #110** — account names scrubbed from paths in logs *and* the bundle's
  `configuration.json`; help text no longer overclaims.
- **#109** — webview CSP (it previously ran with no policy at all). Emitted via
  SvelteKit hash mode rather than `tauri.conf.json`, because SvelteKit's inline
  bootstrap hash changes every build; `csp.test.ts` guards it. (An earlier
  revision of this file added "and CI now builds the frontend" — that is not
  true of the frontend CI job; see the verification gates below.)
- **#111** — untracked the installer artifacts committed in #110.
- **#116** — the archive browser built its tree before the last batch arrived.
- **#115, #118, #120** — smoke harness: selective-restore assertion, evidence
  preservation on failure, decoded exit codes, ASCII guard in CI, and three of
  the four #119 blockers.
- **#117** — 0.3.1 release. **#121** — handoff docs.
- Also closed: app-wide `:focus-visible` (WCAG 2.4.7), borg-core log events
  being dropped below ERROR, and issues #100–#104.

## 0.3.0 line (2026-07-03 → 07-29)

Feature baseline **#71–#80**: backup coverage wizard and canonical selection ·
restore search/version preview and sample-restore drills · resource-aware
scheduling (snooze, wake, Wi-Fi, battery, removable triggers) · append-only
repository hardening · unified protection health and opt-in reporting ·
primary/secondary destinations under one logical backup · Windows
cloud-placeholder detection and hydration policy · aggregate storage/performance
metrics and forecasting · recovery-readiness workflow · versioned built-in
profile templates.

Then: **#82** 0.3.0 bump + a Windows-only `removable.rs` compile break + v0.2→0.3
migration tests · **#83** the `windows-latest` CI job (plus the `network.rs` /
`ssh.rs` fixes it immediately surfaced) · **#84** docs.

**Two release blockers, both of which had also shipped in v0.2.0:**

- **#85 — crt-static.** The installed `borg-ui.exe` failed to launch on clean
  Windows with `0xC0000135` (no VC++ runtime, no CRT bundled). Fixed via
  `.cargo/config.toml` `+crt-static`.
- **#86 — dev-mode.** Post-#56 installers shipped a `cargo build --release`
  binary that loads the Vite dev server, so the app showed "localhost refused to
  connect" instead of its UI. Fixed by building the release exe with
  `pnpm tauri build --no-bundle`.

Both were invisible because `validate-installer` only ever ran `borg.exe` — the
gap described under "Harness gaps" above, still open.

**Why the version bump exposed work:** the 0.3.0 Release dry-run was the first
Windows build since #71–#80 merged, and Linux CI only compiles the
`cfg(not(windows))` stubs. #83 closed that structural gap.

**Connection UX overhaul (2026-07-29):** Vorta-style paste of a full repo address
(`ssh://user@host:port/path` auto-splits into fields, option-like components
refused up front), per-field examples, a "Check repository" summary, and a
plain-language hint layer under raw ssh/borg errors
(`app-tauri/src/lib/connection-hints.ts`, context-scoped). Extended to
Backup/Archives/restore and dashboard history rows in #95 — history hints
deliberately never use ssh context, since events don't record their transport.
Settings decomposed into Connection/Init/Passphrase sections with shared state
in `app-tauri/src/lib/stores/repo-form.svelte.ts`; a 3-step first-run wizard at
`/setup` reuses them. Frontend CI switched npm → pnpm (the stale
`package-lock.json` would have failed `npm ci`) with a vitest gate.

**Smoke against the real backup server (2026-07-29):** the borg 1.4.4 client
from this repo's pinned binary ran init → create → extract → byte-verify →
wrong-passphrase-rejection → delete against a scratch repo on the production
borg 1.2.8 server, restricted-path wrapper honored.

## Audit (2026-07-04), fully closed

A read-only features/UI/security audit of 0.3.0-dev. The codebase came out well:
complete v0.3 feature set, parameterized SQL, secrets in Credential Manager,
correct `unsafe` FFI, signed updater, minimal capabilities. **All findings are
fixed** — re-verified against `git log` on 2026-07-30.

Release-blocker tier:

1. **SSH argument-injection → RCE.** `SSH_FORBIDDEN` blocked metacharacters but
   not a leading `-`, so `ssh_user = -oProxyCommand=calc.exe` reached `ssh` as an
   executed option, reachable by importing a malicious profile;
   `test_ssh_connection` validated nothing. Fixed by #88 — the
   `reject_option_like` gate, wired into `RepoConfig::validate()` and
   independently into `test_ssh_connection`.
2. **Cross-profile prune data loss.** Auto-prune after every backup passed no
   `--glob-archives`, so two profiles sharing a repo could prune each other's
   archives. Fixed by #89, plus a backstop rejecting unscoped globs
   (`prune_refuses_unscoped_archive_globs`).

HIGH tier, all fixed: no `--` end-of-options before borg positional paths (the
`EndOfOptions` trait, same PR as #1) · passphrase rotation desync (#99) ·
generated SSH key not ACL-restricted on Windows (#91, fail-closed) · no
missed-backup catch-up, `<StartWhenAvailable>` missing from generated Task
Scheduler XML (#92) · undefined `--color-error` CSS token, so a failed integrity
check rendered gray not red (#90) · no keyboard focus styles anywhere (#97).

Full Top-10 detail lives in the `borgui-audit-2026-07` memory.
