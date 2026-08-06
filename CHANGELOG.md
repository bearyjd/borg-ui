# Changelog

All notable changes to BorgUI are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.2] - 2026-08-06

### Fixed

- **"Test connection" can no longer hang the app against an unreachable SSH
  host.** The probe trusted ssh's own `-o ConnectTimeout`, which Windows
  OpenSSH does not reliably honour — against a filtered port that silently
  drops packets it was still running after 90 seconds, and nothing else bounded
  the call, so the button simply never came back. The probe is now capped at 30
  seconds, comfortably above ssh's own 10-second connect timeout so a healthy
  but slow host still succeeds, and it reports an honest timeout instead of
  hanging. The `ssh` process is also reaped when that bound fires, so repeated
  attempts no longer leave stray processes behind. (#134)

## [0.3.1] - 2026-08-01

### Security

- **The repository passphrase can now actually be changed.** "Change passphrase"
  previously overwrote only the copy in Windows Credential Manager and never
  told the repository, so the two silently diverged and every later backup and
  restore failed to unlock it. The change flow now runs
  `borg key change-passphrase` first and updates the stored copy only after the
  repository accepts the rotation. Both partial-failure states are reported
  honestly rather than guessed at: a keychain write that fails after a
  successful rotation, and a rotation that timed out and may still be applied.
  (#99)
- **The webview now has a Content-Security-Policy.** It previously ran with no
  policy at all. Nothing in the app injects HTML, so this was not exploitable,
  but every Tauri command is callable from any script in the webview — and
  those now include changing a repository's encryption passphrase. (#109)
- **`BORG_NEW_PASSPHRASE` is redacted** from logs and support bundles. The
  existing pattern could not match it, so the passphrase used during a rotation
  was not covered. (#99)
- **Account names are removed from paths** in logs and in the support bundle's
  `configuration.json`. Bundles can still name individual files a backup failed
  to read — that is what makes the logs useful — and the Diagnostics section now
  says so instead of claiming otherwise. (#108, #110)
- **A stored passphrase is verified against the repository** before being saved,
  and is rejected only on a definite wrong-passphrase verdict, so a passphrase
  can still be stored before the repository exists. (#107)

### Fixed

- **`authenticated` repositories were created with an empty passphrase.** These
  modes do not encrypt file contents but still have a passphrase-protected key,
  so the first "Set passphrase" stored a value the repository would never
  accept. (#107, #110)
- **Recovery readiness follows passphrase changes.** An exported recovery key
  carries the passphrase current at export time, so rotating afterwards makes it
  stale — readiness previously kept reporting "ready" against a key that would
  lock you out. Importing a key counts as proof again, since an import reverts
  the repository to that key's passphrase. (#106, #110, #113)
- **Importing a recovery key warns when it reverts the passphrase.** The import
  can leave the saved passphrase unable to open the repository, which is the
  worst possible moment to discover it by watching backups fail. (#113)
- **The archive browser could under-report a large archive.** The file tree was
  built before the last batch of entries had arrived, so counts and "Select all"
  could be short — a restore would then quietly write fewer files than asked
  for. An incomplete listing is now called out instead of looking authoritative.
  (#116)
- **borg-core log output reaches the log file.** Without `RUST_LOG` set, only
  errors were recorded, so exported support bundles were effectively empty.
  (#94)
- **A scheduled backup that does not run says why.** Every pre-flight bail-out
  was previously silent — no history event and no log line. (#113)
- A corrupted repository is no longer reported as a wrong passphrase, which
  could have led to discarding a passphrase that was correct. (#110)

### Changed

- Creating an `authenticated` or `authenticated-blake2` repository now requires
  a passphrase. Previously the field was hidden for these modes, which is what
  produced the empty-passphrase repositories above. (#107, #110)

## [0.3.0] - 2026-07-04

### Security

- **SSH option-injection remote code execution** — repository, SSH, path, and
  archive inputs are now validated against option-like values before they reach
  the `borg`/`ssh` command line, so a crafted value (e.g. one starting with
  `-oProxyCommand=`) can no longer smuggle options into the spawned process.
  All borg subcommands also pass `--` before positional arguments as
  end-of-options hardening. (#88)
- **Import-hook remote code execution** — importing a profile from a JSON file
  no longer silently arms its pre/post-backup shell hooks. A malicious or
  tampered profile file could previously execute arbitrary commands on the next
  backup; imported hooks now stay disabled until you re-enable them yourself.
  Imported profiles are fully re-validated on import and save. (#88)
- **Cross-machine prune data loss** — retention pruning is now scoped to the
  current profile's own archives (`--glob-archives` with a per-profile archive
  prefix). Previously, pruning a shared repository could delete archives
  created by other machines or profiles. Legacy pre-0.3 unprefixed archives are
  excluded from pruning with a warning, and a custom archive-name template with
  no unique prefix causes prune to warn and skip instead of falling back to a
  repository-wide prune. (#89)

### Fixed

- Release installers now ship the real production build. Installers produced by
  the previous release pipeline contained a dev-mode binary that tried to load
  the UI from a localhost dev server, showing "localhost refused to connect"
  instead of the app. (#86)
- The app now starts on a clean Windows installation without the Microsoft
  Visual C++ redistributable — the MSVC C runtime is statically linked.
  Previously it failed to launch with error 0xC0000135. (#85)
- Repository paths use native Windows paths. (#69)

### Added

- Backup coverage wizard — checks that the folders that matter are actually
  covered by a profile. (#71)
- Restore confidence center. (#72)
- Resource-aware scheduling. (#73)
- Ransomware resilience wizard. (#74)
- Unified protection health reporting. (#75)
- Primary and secondary backup destinations per profile. (#76)
- Windows cloud placeholder handling (OneDrive and similar files-on-demand). (#77)
- Backup storage and performance forecasting. (#78)
- Recovery readiness workflow. (#79)
- Versioned backup profile templates. (#80)
- Scheduled backups are skipped on metered networks. (#61)

### Changed

- Windows-only code is now compiled and tested on every pull request, not just
  at release time. (#83)
- Config migration from 0.2 to 0.3 is covered by dedicated tests. (#82)
- Release CI validates the Authenticode signing configuration and includes an
  installed-updater smoke harness. (#62, #63)

## [0.2.0] - 2026-06-30

### Added

- Repository integrity checks (`borg check`). (#53)
- Encrypted recovery key export. (#54)
- Consent-based signed updates. (#55)
- Native SSH key generation in BorgUI and guided SSH key onboarding. (#52, #58)
- Scheduled retry reporting. (#57)
- Cancelable archive listing streams. (#59)
- Streamlined repository setup and improved SSH setup diagnostics. (#49, #50)
- Persistence and local diagnostics foundation. (#51)

## [0.1.0] - 2026-06-25

First public release: a native Windows GUI for [BorgBackup](https://www.borgbackup.org/).

### Added

- Backup profiles with selector, migration, and JSON import/export.
- Custom archive naming templates with live preview.
- Scheduled backups with a headless runner and autostart at login.
- VSS (Volume Shadow Copy) snapshots for consistent Windows backups.
- Restore/extract from archives with progress streaming.
- Archive browsing, archive diff, and repository compaction.
- Retention policy and prune UI; archive deletion with confirmation.
- Repository initialization with an encryption picker.
- Passphrase storage in the OS keychain.
- Pre/post-backup command hooks.
- System tray with minimize-to-tray, desktop notifications, and a backup
  history dashboard.
- Exclude patterns for backups and schedules.
- Windows installers (MSI and NSIS) that bundle borg, with headless installer
  validation in CI. (#44, #45)

### Fixed

- Local repositories work on Windows despite borg's drive-letter path bug
  (repo paths are rewritten to UNC form). (#31)
- Borg runs fully non-interactively so prompts cannot hang the app. (#24)
- Console-window flashes on Windows process spawns are suppressed. (#25)

[0.3.2]: https://github.com/bearyjd/borg-ui/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/bearyjd/borg-ui/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/bearyjd/borg-ui/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/bearyjd/borg-ui/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/bearyjd/borg-ui/releases/tag/v0.1.0
