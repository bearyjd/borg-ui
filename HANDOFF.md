# BorgUI handoff

Last updated: 2026-08-01. `master` is at **`f9b03ee`**, version **0.3.1**,
**no open PRs**, three open issues (#64, #114, #119).

> This file is a *living status* file, not a changelog. Everything above the
> "History" heading describes the present. If a claim here disagrees with the
> code, the code wins — run `git log --oneline -- <the cited path>` and fix this
> file. It has gone stale before (see "Trust rules" below).

## Start here: 0.3.1 is built but nobody can get it

`v0.3.1` is tagged and its installers are built, but **the GitHub release is a
DRAFT**. GitHub excludes drafts from `releases/latest`, so `releases/latest`
still resolves to **v0.3.0** and no user receives 0.3.1 until someone clicks
Publish.

Draft assets verified present and correct: MSI + NSIS + both `.sig` +
`latest.json` (version `0.3.1`, correct URL, 416-byte signature).

**Next action:** review the assets on the v0.3.1 draft and publish it.

**But the updater is testable right now — don't wait on the publish to start.**
Earlier revisions of this file claimed the updater path "cannot be exercised
until 0.3.1 is published". That was wrong, and it has probably cost a round of
verification. `validate-updater.ps1` takes `-BaselineInstaller` and
`-ExpectedVersion` as parameters and hardcodes no version; the app polls
`releases/latest`, which today resolves to the **published v0.3.0**. So an
updater-capable v0.2.0 baseline updating to 0.3.0 can be smoke-tested against
the current state. Publishing 0.3.1 gates only the test of delivering *0.3.1
specifically*.

```bash
# Runnable today, against the published v0.3.0:
make -C tests/smoke-windows validate-updater \
  BASELINE_INSTALLER=<an updater-capable 0.2.0 -setup.exe> EXPECTED_UPDATE_VERSION=0.3.0

# After the 0.3.1 draft is published:
make -C tests/smoke-windows validate-updater \
  BASELINE_INSTALLER=<a 0.3.0 -setup.exe> EXPECTED_UPDATE_VERSION=0.3.1
```

### Release state

| Tag | GitHub release | Notes |
|---|---|---|
| `v0.3.1` | **Draft** | built 2026-07-31; blocks delivery, and the 0.3.1-specific updater test (not the updater test in general — see above) |
| `v0.3.0` | **Latest** (published 2026-07-05) | what users currently receive |
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
- **#119 — `keychain_credential_manager` has never executed** in the project's
  life. #118 fixed three blockers (MinGW on PATH, placeholder borg resource,
  PATH for the session-1 task); the remaining one is `0xC0000139
  STATUS_ENTRYPOINT_NOT_FOUND` — cargo emits both arm64 and x64
  `WebView2Loader.dll` and the wrong one loads. Fix: pin the x64 copy on the
  task's PATH ahead of `target\debug`. **Credential Manager is therefore
  unverified on Windows** — covered by unit tests and the Linux `keyring`
  backend only. That is the same posture 0.3.0 shipped in, but it should not be
  obscured by the 39 green checks below.

## Windows verification: what is actually proven

**39 smoke checks reported green (2026-07-31/08-01)** against a production build
on the KVM guest: engine, non-admin, VSS† (including backing up an
exclusively-locked file), scheduled task, tray, GUI flows (nav / profile switch /
restore round-trip / cancel), NSIS + MSI install+uninstall.

**† The VSS entry is disputed — do not count it as proven.** `validate-vss.ps1`
gates on a `history.json` the app no longer writes (see "Harness gaps" below),
so it is not clear how it was recorded green. Treat VSS as unverified until that
is reconciled.

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

- `validate-updater` — **runnable today** against the published v0.3.0 (0.2.0
  baseline → 0.3.0); only the 0.3.1-target run waits on the publish. See the
  commands under "Start here".
- `validate-autostart-login` — reboots the guest.
- Multi-drive edge checks — SKIP without a D: drive; `make edge-all` recreates
  the VM with one.
- Credential Manager on Windows — #119.
- New #71–#80 features (wake / battery / Wi-Fi / removable / USB, SSH and
  secondary destinations, OneDrive placeholders, reporting delivery, recovery on
  a clean VM, all five templates through backup/restore) have **no smoke
  automation**.

## Harness gaps and queued work

**`validate-installer.ps1` never launches `borg-ui.exe`** — verified 2026-08-01.
It asserts the file *exists* beside `borg.exe` and `_internal\python311.dll`,
then exercises the installed **`borg.exe`** through a real round-trip. It never
starts the app. This is exactly the hole that let **two release blockers ship in
v0.2.0** and nearly in 0.3.0 (#85 `0xC0000135`, missing CRT; #86 the installed
exe loading the Vite dev server and showing "localhost refused to connect"), and
it is why `validate-installer` once reported a false 10/10. The 39 green checks
do **not** close it: the GUI-flow scripts drive a loose `tauri build` exe staged
at `C:\borgui-test\target\release\`, never the installed-from-installer layout.
**Add a launch-and-render check to `validate-installer.ps1`.**

**Queued harness plan, items 3–5 of 5** (1–2 shipped in #120):

3. Outcome-based assertions instead of internal shapes.
4. **Loud skips** — a permanently-skipping check currently looks identical to a
   passing one in the summary. That is exactly how #119 stayed invisible for the
   project's whole life. Highest value and the cheapest; do this first.
5. De-duplicate the UIA + session-1 blocks copy-pasted across four scripts.

**`validate-vss.ps1` still gates on `history.json` — verified 2026-08-01, and it
is live, not cosmetic.** The move from JSON history to SQLite was only half
followed through the harness. `validate-gui.ps1` correctly dropped its
`history.json` assertion, but `validate-vss.ps1` clears `$historyPath`
(line 173) and then **polls that same file for the success event** (~line 195),
throwing if it never appears — `borg list` runs only after that gate. The app
does not write that file: `borgui.sqlite3` is the store
(`history.rs:922`), and `history.json` survives in `history.rs` only as a
legacy *read* path for a one-time import (`history.rs:658`). So the poll cannot
succeed and the check fails regardless of whether VSS actually worked.
`tests/smoke-windows/README.md` documents the same stale assertion.

**This contradicts the "39 checks green" line above, which counts VSS as
passing.** Both claims cannot be true as written. Reconcile before trusting
either one: establish how VSS was recorded green against a script that gates on
a file the app never writes. Do not assume the app is broken — the far more
likely answer, given four of five smoke failures last pass were fixtures, is
that the harness result was misread.

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
