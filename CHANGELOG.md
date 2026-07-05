# Changelog

All notable changes to BorgUI are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.3.0]: https://github.com/bearyjd/borg-ui/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/bearyjd/borg-ui/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/bearyjd/borg-ui/releases/tag/v0.1.0
