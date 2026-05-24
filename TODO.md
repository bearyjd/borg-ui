# BorgUI Feature Plan

Roadmap derived from a competitive analysis vs Vorta (the established BorgBackup GUI for macOS/Linux). BorgUI's differentiation is Windows + VSS snapshots + native Task Scheduler — these are features Vorta does not offer. The gaps below are what Vorta provides that BorgUI does not yet.

## Phase 1 — Make it usable (critical gaps)

Without these, BorgUI can create backups but not complete the full lifecycle.

- [x] **Restore/extract from archive** — `borg extract` command in `borg.rs`, Tauri command with progress streaming, restore button + destination picker on archives page _(PR #10)_
- [ ] **Encryption / passphrase UI** — set/change passphrase, store in OS credential manager (Windows Credential Manager), use `BORG_PASSCOMMAND` to pass through
- [x] **Archive deletion** — `borg delete` command, delete button per archive row, confirmation dialog
- [x] **Pruning with retention rules** — `borg prune` with hourly/daily/weekly/monthly/yearly counts, UI in settings
- [x] **Repository initialization** — `borg init` with encryption mode selector, "Create new repo" button in settings

## Phase 2 — Make it trustworthy (high-value gaps)

These turn BorgUI from a foreground tool into a daemon-like backup app users actually leave running.

- [x] **System tray with background operation** — Tauri tray icon, minimize-to-tray, restore on click, "Backup now" menu item
- [x] **Desktop notifications** — success/failure toast notifications, configurable in settings
- [x] **Exclude patterns UI** — backend already supports excludes; add UI on backup + schedule forms with custom + preset patterns (`*.tmp`, `node_modules`, `.git`)
- [ ] **Backup history / event log** — persist event log, display on dashboard with timestamps and outcomes
- [ ] **Multiple profiles** — profile concept (named bundle of repo + sources + excludes + schedule), profile selector in nav

## Phase 3 — Polish (medium/low-value gaps)

Feature parity with Vorta where it pays off.

- [ ] **Archive browsing (tree view)** — `borg list <archive>` with tree UI, selective restore checkboxes
- [ ] **Archive diff** — `borg diff` between two selected archives, tree view of changes
- [ ] **Pre/post backup commands** — run shell commands before/after backup with `$repo_url`, `$archive_name` substitution
- [ ] **Custom archive naming templates** — `{hostname}-{date}` style templates, replace auto-generated names
- [ ] **Autostart at login** — Windows registry entry under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
- [ ] **Repository compaction** — `borg compact` button on archives page (requires borg 1.2+)
- [ ] **Profile import/export** — JSON export/import for portable config

## Not pursuing

These are Vorta features that don't fit BorgUI's Windows niche or aren't worth the effort yet:

- **FUSE mount** — Windows has no FUSE; use `borg extract --list` + tree view instead
- **WiFi allowlist / metered network detection** — low value vs effort
- **BorgBase integration** — vendor-specific; users can paste SSH URL manually
- **In-process scheduler** — Windows Task Scheduler is better; we already use it

## BorgUI advantages over Vorta (keep + promote)

- Windows-native (Vorta doesn't support Windows)
- VSS snapshots for consistent backup of locked files
- Native Windows Task Scheduler integration (survives app being closed)
- Tauri/Svelte = small binary, fast UI vs Python/Qt

---

_Source: market research session 2026-05-20. See conversation history for full competitive analysis._
