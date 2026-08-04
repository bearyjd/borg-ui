# Agent-Native Roadmap — BorgUI

Ranked by **Human-Attention-Saved per Unit of Effort** (HAS/UoE). Each item names
the files to touch, the command(s) to run, and how an agent (or reviewer) checks
"done."

This audit is read-only research; nothing in `crates/`, `app-tauri/`, or CI was
changed. See `CLAUDE.md` (repo root) for the day-to-day agent operating rules
this roadmap feeds into.

---

## Top 5 — immediately actionable

### 1. `HANDOFF.md`'s "Audit (2026-07-04)" section is stale and actively misleading — DONE (2026-07-07)
**HAS/UoE: very high — cheap fix, prevents an agent from re-investigating or
re-fixing already-shipped work, or worse, trusting a false "still vulnerable" claim.**

The section (`HANDOFF.md:200-227`) lists two "🔴 candidate release blockers" (SSH
argument-injection RCE, cross-profile prune data loss) and four HIGH findings,
annotated "**All audit items are findings only — nothing implemented.**" That is
no longer true:

| Finding in HANDOFF.md | Actually fixed by |
|---|---|
| SSH arg-injection RCE (`config.rs:15-17,61-70`) | `9257b75` — `reject_option_like` gate, also wired into `test_ssh_connection` (`app-tauri/src-tauri/src/commands.rs:142-157`) |
| Cross-profile prune data loss (`borg.rs:455-496`) | `85a5ea1` (scoped prune) + `61831c2` (backstop rejecting unscoped globs, see `prune_refuses_unscoped_archive_globs` test at `crates/borg-core/src/borg.rs:1184`) |
| No missed-backup catch-up (`scheduler.rs:97-108`) | `4a7278a` |
| SSH key ACLs unrestricted | `a5a8bf0` |
| Missing `--color-error` CSS token | `61831c2` (CSS token commit) |

~~Only one item from that audit remains genuinely open: no passphrase-rotation/
Credential-Manager desync guard.~~ **That claim is stale — corrected 2026-08-04.**
It was true when written (2026-07-07) but rotation shipped afterwards in **#99**
(`d8c3eaa`, "rotate the repository passphrase, not just the stored copy"):
`BorgClient::change_passphrase` runs real `borg key change-passphrase`
(`crates/borg-core/src/borg.rs:546-576`), and the desync guard exists as
`PASSPHRASE_ROTATED_UNSAVED_PREFIX` / `rotated_unsaved_error` in
`app-tauri/src-tauri/src/commands/passphrase.rs`, which distinguishes
"rotated but not saved" from "indeterminate" so the user is never told a
rotation failed when it may have committed. `set_repo_passphrase` also now lives
in that module, not the long-gone `commands.rs:1798`. **Nothing from that audit
is open.** This entry is exactly the failure mode `CLAUDE.md` warns about — verify
"still open" claims with `git log --oneline -S <symbol> -- <path>` before acting. The "`--`
end-of-options before positional paths" item, which this audit *also* first
listed as open, turned out to already be fixed — it shipped in the same commit
(`9257b75`) as the SSH-RCE fix (see the `EndOfOptions` trait,
`crates/borg-core/src/borg.rs:123-137`), just under a different PR title, so a
path/line-only check would have missed it. A no-keyboard-focus-styles (WCAG)
HIGH finding also turned out to still be open and wasn't previously called out
in this "still open" tally; see `HANDOFF.md`.

**Fix — done 2026-07-07:** rewrote `HANDOFF.md`'s "Audit (2026-07-04)" section
to (a) mark the fixed items as fixed with their commit refs, (b) keep only the
genuinely-open items (passphrase rotation, WCAG focus styles) as "open," and
(c) add a note to the file's own maintenance convention: *before trusting any
"known issue" in this file, run `git log --oneline -- <path>` for the cited
file/line range* — and don't stop at "the exact cited line still says X," since
a fix can land under a different line range in the same commit (see the `--`
end-of-options case above). That convention is also codified in `CLAUDE.md`
(this audit's other deliverable) so agents don't need to discover it by hand.

**Acceptance:** `HANDOFF.md` audit section accurately reflects `git log`
against the five items above; no item is described as unimplemented if a commit
already fixes it. **Met** — verified each of the 6 relevant commits
(`9257b75`, `85a5ea1`, `61831c2`, `4a7278a`, `a5a8bf0`, `61d0bc4`) exists via
`git log`/`git show` and touches the claimed file/behavior.

---

### 2. No hermetic borg binary for agent-run e2e tests — DONE (2026-07-07)
**HAS/UoE: high — currently blocks the *only* real backup→restore verification path in a sandbox with no `borg` on `$PATH` (confirmed: `which borg` → not found here).**

`crates/borg-core/tests/e2e_backup_restore.rs` silently skips every test unless
`BORG_TEST_BIN` is set (`borg_or_skip!()` macro, lines 24-45). An agent with no
network access or install permissions cannot exercise the real create → check →
restore path at all — the single most important regression class for a backup
tool (the AGENTS.md testing note "mock the process for unit tests" already
implies this gap exists, but never resolves it).

**Fix:** add a `scripts/fetch-borg-linux.sh` (or a Justfile/Makefile target)
that downloads a pinned borgbackup Linux static binary (borgbackup publishes
single-file Linux binaries) into `.cache/borg-test-bin/`, and document
`BORG_TEST_BIN=$(pwd)/.cache/borg-test-bin/borg` in `README.md`'s e2e section
(`README.md:107-119`) and in the new `CLAUDE.md`. This turns "skip unless a
human has borg installed" into "agent runs one script, then has real e2e
coverage" — no Windows VM required, since `borg-core` is explicitly
platform-agnostic.

**Acceptance:** `BORG_TEST_BIN=<fetched path> cargo test -p borg-core --test
e2e_backup_restore` passes without a human pre-installing anything. **Met** —
added `scripts/fetch-borg-linux.sh` (downloads borgbackup 1.4.4
`borg-linux-glibc231-x86_64`, sha256-pinned, idempotent re-run, into
`.cache/borg-test-bin/`, which is now `.gitignore`d), documented
`BORG_TEST_BIN=$(pwd)/.cache/borg-test-bin/borg` in `README.md`'s e2e section
and in `CLAUDE.md`. Ran it in this sandbox — network access worked here (not
guaranteed in every agent sandbox, so the script is still the right shape even
where it can't run) — and all 12 tests in `e2e_backup_restore.rs` passed:
`encrypted_repository_key_exports_and_imports`,
`special_character_filename_roundtrips`,
`diff_reports_added_removed_and_modified`,
`selective_restore_extracts_only_requested_paths`,
`repository_metadata_and_data_checks_pass`,
`unreadable_file_yields_warning_not_failure`, `compact_runs_after_delete`,
`encrypted_roundtrip_with_passphrase`,
`unencrypted_roundtrip_preserves_file_contents`,
`scoped_prune_never_touches_other_machines_archives`,
`streaming_list_matches_collected_listing`,
`prune_and_delete_manage_archives`.

---

### 3. No fixture/synthetic bug-report → repro harness
**HAS/UoE: high — every past regression in `HANDOFF.md`'s delivered roadmap (#82, #85, #86) was found by a human running the app on a real Windows VM, not from a codified repro.**

There's no `tests/fixtures/` or scripted way to turn "user says restore silently
skipped file X" into a runnable, deterministic case. The two crates with the
most user-facing risk (`borg-core::borg`, `borg-core::archive`) have solid unit
tests but rely on hand-built `RepoConfig`/`BackupProfile` literals scattered
per-test (e.g. `local_repo()`/`profile()` in `e2e_backup_restore.rs:47-64`) with
no shared builder module.

**Fix:** extract a `crates/borg-core/tests/support/mod.rs` (or a `dev-dependencies`
crate) exposing `test_repo()`, `test_profile()`, `sample_archive_tree(dir)`
builders, reusable across `config_persistence.rs`, `e2e_backup_restore.rs`,
`validation_pipeline.rs`, and future regression tests. Then add a short
`docs/reproducing-bug-reports.md` describing the pattern: *given a bug report
naming a profile/config shape, write a `#[tokio::test]` in
`e2e_backup_restore.rs` using these builders before touching implementation
code* — i.e., codify the TDD loop the repo's `AGENTS.md` and `testing.md`
already mandate but never spells out mechanically for this domain.

**Acceptance:** a new regression test can be written in under 10 lines using
the shared builders instead of hand-rolling `RepoConfig`/`BackupProfile` boilerplate.

---

### 4. `validate-installer.ps1` doesn't launch `borg-ui.exe` (already a known gap, but unactioned)
**HAS/UoE: high — HANDOFF.md itself flags this (`HANDOFF.md:132-134`) as the exact reason #85/#86 (both release blockers) went undetected for two releases, yet no issue or plan file tracks it.**

`tests/smoke-windows/validate-installer.ps1` only exercises the bundled
`borg.exe` + engine round-trip, never the app binary itself. This is the
single highest-leverage smoke-test gap: it would have caught both #85
(`0xC0000135` crt-static crash) and #86 (dev-server URL bug) automatically,
pre-release, with no human running the installed app by hand.

**Fix:** add a launch-and-probe step to
`tests/smoke-windows/validate-installer.ps1` — start `borg-ui.exe --minimized`
(or equivalent), poll for the main window/tray icon to appear within N seconds,
capture stdout/stderr, and fail the script if the process exits non-zero or a
window never appears. Wire it into `make validate-installer` (already the
existing target — `tests/smoke-windows/Makefile:91`) so it runs automatically
instead of needing a human to eyeball the VM.

**Acceptance:** `make -C tests/smoke-windows validate-installer` fails loudly
if a build reproduces the #85 or #86 class of bug (dev-server error page or
DLL-load crash), without a human watching the VM screen.

---

### 5. `CLAUDE.md` didn't exist — agents had no single-file operating doc distinct from `AGENTS.md`
**HAS/UoE: high — one-time authoring cost, saves re-discovery of workspace conventions on every session.**

`AGENTS.md` (root + 6 nested files) already documents *structure* (what each
directory is for) well. It does not capture the *procedural* knowledge an
autonomous agent needs before touching code: which commands are safe to run
locally vs. which need Windows CI, the doc-staleness trap above, the
option-injection convention (`reject_option_like` — every new untrusted argv
string must go through it, not just SSH fields), or the security-invariant
checklist in `HANDOFF.md` (never log passphrases/keys/paths).

**Fix:** done — see `CLAUDE.md` at repo root, written this pass. It
cross-references `AGENTS.md` rather than duplicating it.

**Acceptance:** `CLAUDE.md` exists, cites only verified commands (cross-checked
against `Cargo.toml`, `README.md`, `.github/workflows/ci.yml`, and
`.claude/settings.local.json`'s allowlist), and does not restate `AGENTS.md`'s
structural tables.

---

## Audit detail

### A. Human-judgment chokepoints

`AGENTS.md` (7 files) covers structure and dependency direction thoroughly —
crate boundaries, IPC command wiring, design tokens. What it does *not* cover,
and what currently lives only in reviewers' heads or scattered commit messages:

- **The option-injection convention.** `reject_option_like` (`crates/borg-core/src/config.rs:25-36`)
  is the load-bearing security gate for every untrusted string that reaches an
  argv position (SSH host/user, repo path, and — per the code comment — *any
  future* such field). Nothing tells an agent adding a new field (e.g. a
  future `borg_binary_path` override) that it must run through this gate.
  Codified in `CLAUDE.md`.
- **Doc staleness.** As shown in item 1, `HANDOFF.md` can lag `git log` by
  several commits while still asserting "nothing implemented." No CI check or
  convention catches this. Codified in `CLAUDE.md` as "verify HANDOFF.md claims
  against git log before trusting them."
- **Security/privacy invariants** (`HANDOFF.md:143-153`: never log passphrases,
  SSH keys, recovery payloads, source listings, restore paths) are documented
  but only in `HANDOFF.md`, a file whose primary purpose is "current release
  status," not "durable invariant." An agent skimming only `AGENTS.md` +
  `CLAUDE.md` could miss this. Pulled forward into `CLAUDE.md` directly.
- **What CI actually proves vs. what it can't**: the Linux job never compiles
  `cfg(windows)` code (VSS, scheduler, autostart, cloud placeholders); the
  Windows CI job compiles and unit-tests it but never runs a real Windows
  smoke flow (that's `tests/smoke-windows/`, gated behind a KVM host). An
  agent could reasonably believe "CI green" implies "Windows behavior
  verified" — it only implies "Windows compiles and unit-tests pass."

### B. Verification gaps

- **No borg binary in a typical sandbox** (see item 2) — blocks the one real
  integration-test path that exists.
- **No Windows execution environment for `cfg(windows)` code** short of the
  `tests/smoke-windows/` KVM harness, which needs a bare-metal host with
  `/dev/kvm`, `sshpass`, and ~20GB disk — not available in most agent sandboxes.
  `crates/borg-platform-win`'s path-planning logic (VSS junction paths,
  scheduler task XML construction) is unit-testable cross-platform per its own
  `AGENTS.md`, but the *effectful* half (actually calling `vssadmin`/`schtasks`)
  has zero non-Windows-VM verification path. Nothing wraps `schtasks.exe`/VSS
  COM calls behind a swappable trait for unit-level mocking — that would let
  an agent verify scheduler/VSS *logic* (argument construction, error mapping)
  without a VM at all.
- **Tauri UI has no automated check** reachable from a headless agent sandbox.
  `pnpm check` (type-check) and `pnpm build` run in CI, but there is no
  Playwright/component-test layer for the Svelte routes (`app-tauri/src/routes/`)
  — visual/interaction regressions (the `--color-error` CSS token bug that
  shipped, per `HANDOFF.md:227`) are only caught by the Windows GUI smoke
  scripts, which need a display and a production Tauri build.
- **Test counts are asserted in `HANDOFF.md` prose** ("145 borg-core tests, 58
  borg-platform-win tests, 85 app-backend tests") rather than derivable from a
  single command — an agent has no easy `cargo test --workspace -- --list`
  baseline to diff against for "did I add/break tests." (This audit didn't run
  a full test listing per the task's build/test avoidance constraint, but
  `cargo test --workspace -- --list` would produce it cheaply.)

### C. Reproduction paths

- **No shared test-fixture builders** (see item 3) — every test file
  hand-constructs `RepoConfig`/`BackupProfile`, increasing the cost of writing
  a new regression test from a bug report.
- **No synthetic large-archive fixture reusable outside the Windows VM.** The
  100k-entry archive-streaming/virtualization test only exists as
  `validate-archive-smoke.ps1` on a live VM; there's no way to generate a
  large-archive fixture and unit-test the streaming/pagination logic in
  `borg-core`/`app-tauri` directly.
- **`validate-installer` doesn't launch the app** (item 4) — the exact gap that
  let two release blockers ship silently across a version bump.
- **No golden/replay corpus for borg's JSON progress output** (`progress.rs`).
  `ProgressEvent` deserialization is presumably unit-tested against hand-built
  JSON, but there's no captured-from-real-borg corpus of progress/log-json
  lines to replay against parser changes — the parser is the part most likely
  to silently regress against a new borg version.

### D. Structural obstacles

The crate boundaries are clean and explicit (`borg-core` platform-agnostic →
`borg-platform-win` Windows-only → `app-tauri/src-tauri` ties both together;
enforced by `crates/AGENTS.md`'s stated dependency direction, and `borg-core`
has zero internal dependencies per `cargo metadata`). No entanglement found
there. The one soft spot:

- ~~**`app-tauri/src-tauri/src/commands.rs` is a wide single file**~~ —
  **RESOLVED in #128 (2026-08-04).** It had reached 2,781 lines and 86 commands,
  well past the 800-line convention in `coding-style.md`. Split by domain into
  16 modules under `app-tauri/src-tauri/src/commands/` (largest now 405 lines),
  which is roughly the split this entry proposed. `mod.rs` re-exports every
  command, so Tauri's command-registration model is unaffected and the
  `generate_handler!` list in `lib.rs` did not change. Both concerns this entry
  raised are addressed: "add a new command" now means editing the module for
  that domain, and "review security-sensitive IPC" can be scoped to one file
  (e.g. the SSH option-injection gate is `commands/ssh.rs`, ~85 lines, instead
  of buried in a 2,781-line file).
