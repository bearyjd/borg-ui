# BorgUI Roadmap Status

Last updated: 2026-08-06.

The original Vorta-parity roadmap is complete for the Windows-focused v0.1 line:

- backup destinations: SSH, local folder, USB drive, and network share
- VSS snapshots for consistent Windows backups of open files, with live-file fallback
- restore, selective restore, archive browsing, archive diff, and archive-list cancellation
- repository initialization, encryption/passphrase storage, retention/prune, compact, and delete
- multiple profiles, profile import/export, custom archive naming, pre/post hooks
- backup history, SQLite diagnostics/history storage, desktop notifications, tray, and autostart
- scheduled backups through Windows Task Scheduler, including transient retry and missed-run reporting
- repository integrity checks, manual full-data verification, and opt-in monthly metadata checks
- encrypted portable recovery-key export/import
- consent-based signed updater flow
- Windows release workflow, unsigned artifacts by default, and signing-ready Azure Trusted Signing path
- guided SSH public-key onboarding without password collection
- opt-in metered-network skipping for scheduled backups
- installed-app updater smoke harness
- tested Azure signing configuration preflight

## Current release posture

- **`v0.3.2` is the published latest** (published 2026-08-06; MSI, NSIS, both
  updater signatures, `latest.json`). `releases/latest` resolves to it, so
  eligible installed clients are offered it.
- The installed-app updater smoke **has been re-run for 0.3.2** and passed on
  the Windows KVM guest: installed v0.3.1 → update prompt → user confirmation →
  installed v0.3.2 (3 passed, 0 failed, 0 skipped, 2026-08-06). It cannot run
  before publishing — the updater endpoint is hardcoded to `releases/latest`.
  `validate-updater.ps1` takes the baseline and expected version as parameters
  and hardcodes no version, so retain it for future release checks.
- `validate-installer` passed **12/0/0** against the v0.3.2 artifacts before
  they were published, including the interactive render checks on both NSIS and
  MSI.
- **`master` and the `v0.3.2` tag are level right now** (the tag points at
  #140, the tip), so nothing is waiting to ship. That is the exception here,
  not the rule — check `git log --oneline v0.3.2..master` rather than trusting
  this bullet.
- Borg-for-Windows 1.4.4+win7 fixes native drive-letter repositories; BorgUI now
  passes those paths directly, including for standard users.
- Installers remain usable unsigned. Authenticode signing is prepared but intentionally disabled until Azure Trusted Signing repository configuration exists.
- Updater signing is separate from Authenticode signing; keep the updater private key only in GitHub Actions secrets.

Shipped since 2026-07-02 (see `HANDOFF.md` History → "0.3.0 line"): Vorta-style
connection UX with paste-to-fill and repository summary, context-scoped
plain-language error hints across Settings/Backup/Archives/dashboard, settings
page decomposition, first-run setup wizard at `/setup`, frontend CI switched to
pnpm with a vitest gate.

## Tracked follow-up issues

- [#64](https://github.com/bearyjd/borg-ui/issues/64) — production Authenticode signing. Repo side complete; blocked on Azure provisioning. Runbook + exact missing secrets/variables are in a comment on the issue.
- [#114](https://github.com/bearyjd/borg-ui/issues/114) — archive browser intermittently under-counts (~1 run in 6). [#116](https://github.com/bearyjd/borg-ui/pull/116) fixes the race it is attributed to and makes an incomplete listing visible. **17 consecutive clean `validate-archive-smoke` runs against a production v0.3.1 build (2026-08-05, posted to the issue): count assertion 17/17, zero `incomplete` banners, and each run logged a progressive count mid-stream so it genuinely entered the race window.** At the documented ~1-in-6 rate that is a (5/6)^17 ≈ 4.5% fluke. Still open deliberately: controlled repetition on one machine and one archive shape is not the *varied field exposure* the closure criterion asks for. The disposition call is the maintainer's.

**Release state: `v0.3.2` is published and is `releases/latest`** (2026-08-06).
The complete public release has MSI + NSIS + both `.sig` files + `latest.json`
(version `0.3.2`, correct URL, 416-byte signature), and the v0.3.1 → v0.3.2
updater path has passed on the Windows guest. 0.3.2 is a one-fix patch release:
#134, the bounded SSH connection probe. A stale duplicate **draft** of v0.3.0
still sits beside the published v0.3.0; it is harmless.

Closed in the 0.3.1 cycle: #100, #101, #102, #103, #104 (all raised by the
security and adversarial review passes on #99), plus the passphrase-change
desync, app-wide `:focus-visible`, and borg-core log events being dropped
below ERROR. [#119](https://github.com/bearyjd/borg-ui/issues/119) is also
closed (2026-08-03): the `WebView2Loader.dll` blocker was resolved and
**Credential Manager is now verified on Windows** — the session-1
`borg-platform-win` test passed set → fresh get → `cmdkey` visibility → clear.

Provider-specific SSH examples and Windows archive mounting were evaluated in
[#67](https://github.com/bearyjd/borg-ui/issues/67). There is no recorded user
demand beyond the gate issue itself. Borg-for-Windows does not provide
`borg mount`; WinFsp is a maintained filesystem framework, not a Borg archive
adapter. Do not add provider-specific text or a filesystem-driver dependency
without a new issue containing concrete demand and a maintained, tested design.
Browse/selective restore remains the supported archive access path.

## Quality gate for future PRs

Run the relevant focused tests plus:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cd app-tauri && pnpm check && pnpm test && pnpm build
git diff --check
```

`pnpm test` is required — the frontend CI job runs it and will fail without it.
That job does not run `pnpm build`; keep it here anyway to catch a broken
production bundle before a release.

For release-affecting changes, also run the applicable Windows smoke command from
`tests/smoke-windows/README.md` and a Release workflow dry run.
