<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open, save as saveDialog } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import { repoState, isLocalRepo, type RepoConfig } from '$lib/stores/repo.svelte';
  import { scheduleState, nextRun, type ScheduleConfig } from '$lib/stores/schedule.svelte';
  import { retentionState, type RetentionConfig } from '$lib/stores/retention.svelte';
  import { notificationsState } from '$lib/stores/notifications.svelte';
  import { profilesState } from '$lib/stores/profiles.svelte';
  import FieldHelp from '$lib/components/FieldHelp.svelte';

  type RepoType = 'ssh' | 'local';

  let repoType = $state<RepoType>('ssh');
  let sshHost = $state('');
  let sshPort = $state(22);
  let sshUser = $state('');
  let repoPath = $state('');
  let sshKeyPath = $state('');
  let testing = $state(false);
  let saving = $state(false);
  let testResult = $state('');
  let saveResult = $state('');

  // For a local repo, "configured" means a folder path is filled in. For SSH,
  // we need host + user + path. Used to enable Save/Init/Prune/passphrase.
  let repoConfigured = $derived(
    repoType === 'local'
      ? repoPath.trim() !== ''
      : sshHost.trim() !== '' && repoPath.trim() !== ''
  );

  async function browseLocalRepoFolder() {
    const selected = await open({ directory: true, multiple: false, title: 'Select backup folder' });
    if (selected) repoPath = selected as string;
  }

  const EXCLUDE_PRESETS = ['*.tmp', '*.cache', 'node_modules', '.git', 'target', '__pycache__', '.venv', 'dist', 'build'];

  let newProfileName = $state('');
  let profileResult = $state('');

  let archiveTemplate = $state('');
  let archivePreview = $state('');
  let archiveTemplateResult = $state('');
  let archiveTemplateSaving = $state(false);

  async function refreshArchivePreview() {
    try {
      archivePreview = await invoke<string>('preview_archive_name', { template: archiveTemplate });
    } catch (e) {
      archivePreview = `(${e})`;
    }
  }

  async function saveArchiveTemplate() {
    const id = profilesState.activeId;
    if (!id) {
      archiveTemplateResult = 'No active profile';
      return;
    }
    archiveTemplateSaving = true;
    try {
      const template = archiveTemplate.trim();
      await invoke('set_profile_template', { id, template: template || null });
      await profilesState.load();
      archiveTemplateResult = template ? 'Template saved' : 'Template reset to default';
    } catch (e) {
      archiveTemplateResult = `Failed: ${e}`;
    } finally {
      archiveTemplateSaving = false;
    }
  }

  async function addProfile() {
    const name = newProfileName.trim();
    if (!name) {
      profileResult = 'Name required';
      return;
    }
    const repo = currentRepoFromForm();
    if (!repo) {
      profileResult = repoType === 'local'
        ? 'Choose a backup folder first'
        : 'Fill in SSH host, user, and repo path first';
      return;
    }
    try {
      const created = await profilesState.create(name, repo);
      await profilesState.setActive(created.id);
      await repoState.load();
      newProfileName = '';
      profileResult = `Created profile "${name}"`;
    } catch (e) {
      profileResult = `Failed: ${e}`;
    }
  }

  let renameModalOpen = $state(false);
  let renameInput = $state('');
  let deleteModalOpen = $state(false);

  function openRenameModal() {
    if (!profilesState.activeId) return;
    renameInput = profilesState.active?.name ?? '';
    renameModalOpen = true;
  }

  async function confirmRename() {
    const id = profilesState.activeId;
    const newName = renameInput.trim();
    if (!id || !newName) {
      renameModalOpen = false;
      return;
    }
    try {
      await profilesState.rename(id, newName);
      profileResult = `Renamed to "${newName}"`;
    } catch (e) {
      profileResult = `Failed: ${e}`;
    } finally {
      renameModalOpen = false;
    }
  }

  async function exportActive() {
    const id = profilesState.activeId;
    if (!id) return;
    const profile = profilesState.active;
    const suggestedName = `${profile?.id ?? 'profile'}.borgui.json`;
    try {
      const path = await saveDialog({
        title: 'Export profile',
        defaultPath: suggestedName,
        filters: [{ name: 'BorgUI profile', extensions: ['json'] }],
      });
      if (!path) return;
      await invoke('export_profile', { id, path });
      profileResult = `Exported to ${path}`;
    } catch (e) {
      profileResult = `Export failed: ${e}`;
    }
  }

  async function importProfile() {
    try {
      const path = await open({
        title: 'Import profile',
        multiple: false,
        filters: [{ name: 'BorgUI profile', extensions: ['json'] }],
      });
      if (!path) return;
      const imported = await invoke<{ id: string; name: string }>('import_profile', { path });
      await profilesState.load();
      await profilesState.setActive(imported.id);
      await repoState.load();
      profileResult = `Imported profile "${imported.name}"`;
    } catch (e) {
      profileResult = `Import failed: ${e}`;
    }
  }

  function openDeleteModal() {
    if (!profilesState.activeId) return;
    if (profilesState.profiles.length <= 1) {
      profileResult = 'Cannot delete the only profile';
      return;
    }
    deleteModalOpen = true;
  }

  async function confirmDeleteProfile() {
    const id = profilesState.activeId;
    if (!id) {
      deleteModalOpen = false;
      return;
    }
    try {
      await profilesState.remove(id);
      await repoState.load();
      profileResult = 'Profile deleted';
    } catch (e) {
      profileResult = `Failed: ${e}`;
    } finally {
      deleteModalOpen = false;
    }
  }

  // Local mirror of the store value so the checkbox reflects the user's
  // *attempt* even when the OS rejects permission, then we roll it back.
  let notificationsEnabled = $state(notificationsState.enabled);
  let notificationsResult = $state('');

  /** Build a RepoConfig from the current form, honoring the repo type. */
  function buildRepoConfig(): RepoConfig {
    if (repoType === 'local') {
      // The empty-host/empty-user convention IS the local marker — the backend
      // then uses repo_path directly as an on-disk path.
      return {
        ssh_host: '',
        ssh_port: 0,
        ssh_user: '',
        repo_path: repoPath,
        ssh_key_path: null,
      };
    }
    return {
      ssh_host: sshHost,
      ssh_port: sshPort,
      ssh_user: sshUser,
      repo_path: repoPath,
      ssh_key_path: sshKeyPath || null,
    };
  }

  function currentRepoFromForm(): RepoConfig | null {
    if (!repoConfigured) return null;
    if (repoType === 'ssh' && !sshUser) return null;
    return buildRepoConfig();
  }

  async function refreshPassphraseStatus() {
    const repo = currentRepoFromForm();
    if (!repo) {
      hasPassphrase = false;
      return;
    }
    passphraseLoading = true;
    try {
      hasPassphrase = await invoke<boolean>('has_repo_passphrase', { repo });
    } catch {
      hasPassphrase = false;
    } finally {
      passphraseLoading = false;
    }
  }

  function openPassphraseModal() {
    passphraseInput = '';
    passphraseConfirm = '';
    passphraseResult = '';
    passphraseModalOpen = true;
  }

  async function savePassphrase() {
    const repo = currentRepoFromForm();
    if (!repo) {
      passphraseResult = 'Configure SSH connection first.';
      return;
    }
    if (!passphraseInput) {
      passphraseResult = 'Passphrase cannot be empty.';
      return;
    }
    if (passphraseInput !== passphraseConfirm) {
      passphraseResult = 'Passphrases do not match.';
      return;
    }
    passphraseSaving = true;
    try {
      await invoke('set_repo_passphrase', { repo, passphrase: passphraseInput });
      hasPassphrase = true;
      passphraseModalOpen = false;
      passphraseInput = '';
      passphraseConfirm = '';
      passphraseResult = 'Passphrase saved to system keychain.';
    } catch (e) {
      passphraseResult = `Failed to save passphrase: ${e}`;
    } finally {
      passphraseSaving = false;
    }
  }

  let clearPassphraseModalOpen = $state(false);

  async function confirmClearPassphrase() {
    const repo = currentRepoFromForm();
    if (!repo) {
      clearPassphraseModalOpen = false;
      return;
    }
    try {
      await invoke('clear_repo_passphrase', { repo });
      hasPassphrase = false;
      passphraseResult = 'Passphrase removed from keychain.';
    } catch (e) {
      passphraseResult = `Failed to clear passphrase: ${e}`;
    } finally {
      clearPassphraseModalOpen = false;
    }
  }

  async function toggleNotifications(value: boolean) {
    notificationsResult = '';
    try {
      await notificationsState.setEnabled(value);
      notificationsEnabled = notificationsState.enabled;
      notificationsResult = value ? 'Notifications enabled.' : 'Notifications disabled.';
    } catch (e) {
      notificationsEnabled = false;
      notificationsResult = `${e}`;
    }
  }

  let scheduleEnabled = $state(false);
  let scheduleType = $state<'hourly' | 'daily'>('daily');
  let scheduleHour = $state(2);
  let scheduleMinute = $state(0);
  let schedulePaths = $state<string[]>([]);
  let scheduleExcludes = $state<string[]>([]);
  let scheduleExcludeInput = $state('');
  let scheduleSaving = $state(false);
  let scheduleResult = $state('');

  let scheduleNextRunLabel = $derived.by(() => {
    if (!scheduleEnabled) return '';
    const schedule = scheduleType === 'hourly'
      ? { type: 'hourly' as const }
      : { type: 'daily' as const, hour: scheduleHour, minute: scheduleMinute };
    const next = nextRun({ enabled: true, source_paths: [], schedule, excludes: [] });
    if (!next) return '';
    return next.toLocaleString(undefined, {
      weekday: 'short',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  });

  function addScheduleExclude(pattern: string) {
    const trimmed = pattern.trim();
    if (trimmed && !scheduleExcludes.includes(trimmed)) {
      scheduleExcludes = [...scheduleExcludes, trimmed];
    }
    scheduleExcludeInput = '';
  }

  function removeScheduleExclude(index: number) {
    scheduleExcludes = scheduleExcludes.filter((_, i) => i !== index);
  }

  let keepHourly = $state<number | null>(null);
  let keepDaily = $state<number | null>(7);
  let keepWeekly = $state<number | null>(4);
  let keepMonthly = $state<number | null>(6);
  let keepYearly = $state<number | null>(null);
  let retentionSaving = $state(false);
  let retentionPruning = $state(false);
  let retentionResult = $state('');

  let hasPassphrase = $state(false);
  let passphraseLoading = $state(false);
  let passphraseModalOpen = $state(false);
  let passphraseInput = $state('');
  let passphraseConfirm = $state('');
  let passphraseSaving = $state(false);
  let passphraseResult = $state('');

  // Close any open modal with the Escape key. Mirrors the click-backdrop-to-close
  // behaviour so modals are dismissable from the keyboard too.
  $effect(() => {
    const anyOpen =
      renameModalOpen || deleteModalOpen || clearPassphraseModalOpen || passphraseModalOpen;
    if (!anyOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      renameModalOpen = false;
      deleteModalOpen = false;
      clearPassphraseModalOpen = false;
      passphraseModalOpen = false;
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  let initEncryption = $state<'repokey' | 'keyfile' | 'repokey-blake2' | 'keyfile-blake2' | 'authenticated' | 'authenticated-blake2' | 'none'>('repokey-blake2');
  let initPassphrase = $state('');
  let initPassphraseConfirm = $state('');
  let initing = $state(false);
  let initResult = $state('');
  let needsPassphrase = $derived(
    initEncryption !== 'none' &&
    initEncryption !== 'authenticated' &&
    initEncryption !== 'authenticated-blake2'
  );

  $effect(() => {
    const r = repoState.config;
    if (r) {
      repoType = isLocalRepo(r) ? 'local' : 'ssh';
      sshHost = r.ssh_host;
      sshPort = r.ssh_port || 22;
      sshUser = r.ssh_user;
      repoPath = r.repo_path;
      sshKeyPath = r.ssh_key_path ?? '';
    }
  });

  async function testConnection() {
    testing = true;
    testResult = '';
    try {
      const ok = await invoke('test_ssh_connection', {
        host: sshHost,
        port: sshPort,
        user: sshUser,
        keyPath: sshKeyPath || null,
      });
      testResult = ok ? 'Connection successful!' : 'Connection failed.';
    } catch (e) {
      testResult = `Error: ${e}`;
    } finally {
      testing = false;
    }
  }

  async function save() {
    saving = true;
    saveResult = '';
    try {
      await repoState.save(buildRepoConfig());
      saveResult = 'Settings saved.';
    } catch (e) {
      saveResult = `Save failed: ${e}`;
    } finally {
      saving = false;
    }
  }

  async function initRepo() {
    initResult = '';
    if (needsPassphrase) {
      if (!initPassphrase) {
        initResult = 'Passphrase required for this encryption mode.';
        return;
      }
      if (initPassphrase !== initPassphraseConfirm) {
        initResult = 'Passphrases do not match.';
        return;
      }
    }

    initing = true;
    try {
      await invoke('init_repo', {
        repo: buildRepoConfig(),
        encryption: initEncryption,
        passphrase: needsPassphrase ? initPassphrase : null,
      });
      initResult = 'Repository initialized successfully.';
      initPassphrase = '';
      initPassphraseConfirm = '';
      await refreshPassphraseStatus();
    } catch (e) {
      initResult = `Init failed: ${e}`;
    } finally {
      initing = false;
    }
  }

  onMount(async () => {
    try {
      await scheduleState.load();
      if (scheduleState.config) {
        scheduleEnabled = scheduleState.config.enabled;
        schedulePaths = [...scheduleState.config.source_paths];
        scheduleExcludes = [...(scheduleState.config.excludes ?? [])];
        if (scheduleState.config.schedule.type === 'hourly') {
          scheduleType = 'hourly';
        } else {
          scheduleType = 'daily';
          scheduleHour = scheduleState.config.schedule.hour;
          scheduleMinute = scheduleState.config.schedule.minute;
        }
      }
    } catch {
      // No schedule config yet
    }

    try {
      await retentionState.load();
      if (retentionState.config) {
        keepHourly = retentionState.config.keep_hourly;
        keepDaily = retentionState.config.keep_daily;
        keepWeekly = retentionState.config.keep_weekly;
        keepMonthly = retentionState.config.keep_monthly;
        keepYearly = retentionState.config.keep_yearly;
      }
    } catch {
      // No retention config yet
    }

    await refreshPassphraseStatus();
  });

  let lastActiveId = $state<string | null>(profilesState.activeId);
  $effect(() => {
    const id = profilesState.activeId;
    if (id === lastActiveId) return;
    lastActiveId = id;

    if (scheduleState.config) {
      scheduleEnabled = scheduleState.config.enabled;
      schedulePaths = [...scheduleState.config.source_paths];
      scheduleExcludes = [...(scheduleState.config.excludes ?? [])];
      if (scheduleState.config.schedule.type === 'hourly') {
        scheduleType = 'hourly';
      } else {
        scheduleType = 'daily';
        scheduleHour = scheduleState.config.schedule.hour;
        scheduleMinute = scheduleState.config.schedule.minute;
      }
    } else {
      scheduleEnabled = false;
      scheduleType = 'daily';
      scheduleHour = 2;
      scheduleMinute = 0;
      schedulePaths = [];
      scheduleExcludes = [];
    }

    if (retentionState.config) {
      keepHourly = retentionState.config.keep_hourly;
      keepDaily = retentionState.config.keep_daily;
      keepWeekly = retentionState.config.keep_weekly;
      keepMonthly = retentionState.config.keep_monthly;
      keepYearly = retentionState.config.keep_yearly;
    } else {
      keepHourly = null;
      keepDaily = 7;
      keepWeekly = 4;
      keepMonthly = 6;
      keepYearly = null;
    }

    archiveTemplate = profilesState.active?.archive_template ?? '';
    refreshArchivePreview();
    refreshPassphraseStatus();
  });

  $effect(() => {
    void archiveTemplate;
    refreshArchivePreview();
  });

  function currentRetention(): RetentionConfig {
    return {
      keep_hourly: keepHourly,
      keep_daily: keepDaily,
      keep_weekly: keepWeekly,
      keep_monthly: keepMonthly,
      keep_yearly: keepYearly,
    };
  }

  async function saveRetention() {
    retentionSaving = true;
    retentionResult = '';
    try {
      await retentionState.save(currentRetention());
      retentionResult = 'Retention policy saved.';
    } catch (e) {
      retentionResult = `Save failed: ${e}`;
    } finally {
      retentionSaving = false;
    }
  }

  async function runPrune() {
    retentionPruning = true;
    retentionResult = '';
    try {
      const warnings = await invoke<string[]>('prune_repo', {
        repo: buildRepoConfig(),
        retention: currentRetention(),
      });
      retentionResult =
        warnings.length > 0
          ? `Prune completed with ${warnings.length} warning${warnings.length === 1 ? '' : 's'}: ${warnings.join('; ')}`
          : 'Prune completed successfully.';
    } catch (e) {
      retentionResult = `Prune failed: ${e}`;
    } finally {
      retentionPruning = false;
    }
  }

  async function addScheduleFolder() {
    const selected = await open({ directory: true, multiple: false, title: 'Select folder for scheduled backup' });
    if (selected && !schedulePaths.includes(selected as string)) {
      schedulePaths = [...schedulePaths, selected as string];
    }
  }

  async function saveSchedule() {
    scheduleSaving = true;
    scheduleResult = '';
    try {
      const schedule = scheduleType === 'hourly'
        ? { type: 'hourly' as const }
        : { type: 'daily' as const, hour: scheduleHour, minute: scheduleMinute };
      const config: ScheduleConfig = {
        enabled: scheduleEnabled,
        source_paths: schedulePaths,
        schedule,
        excludes: scheduleExcludes,
      };
      await scheduleState.save(config);
      scheduleResult = scheduleEnabled ? 'Schedule saved and activated.' : 'Schedule disabled.';
    } catch (e) {
      scheduleResult = `Schedule save failed: ${e}`;
    } finally {
      scheduleSaving = false;
    }
  }
</script>

<div class="settings-page">
  <header class="page-header">
    <h1>Settings</h1>
    <p class="subtitle">Repository and connection configuration</p>
  </header>

  <form class="settings-form" onsubmit={(e) => { e.preventDefault(); addProfile(); }}>
    <fieldset class="form-group">
      <legend>Profiles</legend>
      <FieldHelp
        text="A profile bundles one backup destination together with its schedule and retention rules. You only need more than one if you back up different things to different places. Switch between them with the picker in the sidebar."
        examples={[
          { input: 'Documents → office server' },
          { input: 'Photos → USB drive' },
        ]}
      />

      {#if profilesState.active}
        <div class="profile-current">
          <span class="field-label">Active</span>
          <code>{profilesState.active.name}</code>
          <div class="profile-actions">
            <button type="button" class="btn btn-secondary" onclick={openRenameModal}>Rename</button>
            <button type="button" class="btn btn-secondary" onclick={exportActive}>Export</button>
            <button type="button" class="btn btn-secondary" onclick={openDeleteModal} disabled={profilesState.profiles.length <= 1}>Delete</button>
          </div>
        </div>
      {/if}

      <div class="field">
        <label for="new-profile-name">New profile name</label>
        <div class="inline-row">
          <input id="new-profile-name" type="text" bind:value={newProfileName} placeholder="e.g. Work laptop" />
          <button type="submit" class="btn btn-primary">+ Add</button>
          <button type="button" class="btn btn-secondary" onclick={importProfile}>Import…</button>
        </div>
      </div>

      {#if profileResult}
        <div class="test-result" class:success={profileResult.includes('Created') || profileResult.includes('Renamed') || profileResult.includes('deleted') || profileResult.includes('Exported') || profileResult.includes('Imported')} class:error={profileResult.includes('failed') || profileResult.includes('Failed') || profileResult.includes('required') || profileResult.includes('Cannot')}>
          {profileResult}
        </div>
      {/if}
    </fieldset>
  </form>

  {#if profilesState.active}
    <form class="settings-form" onsubmit={(e) => { e.preventDefault(); saveArchiveTemplate(); }}>
      <fieldset class="form-group">
        <legend>Archive Naming</legend>
        <FieldHelp text="Each backup is saved under a name built from this template. Leave it blank to use the sensible default. Variables you can use:" />
        <ul class="var-help">
          <li><code>{'{date}'}</code> <span>today's date, e.g. 2026-05-31</span></li>
          <li><code>{'{time}'}</code> <span>the time, e.g. 143015</span></li>
          <li><code>{'{datetime}'}</code> <span>date and time together</span></li>
          <li><code>{'{hostname}'}</code> <span>this PC's name, e.g. her-pc</span></li>
          <li><code>{'{profile}'}</code> <span>the active profile name</span></li>
          <li><code>{'{random}'}</code> <span>a few random characters, keeps names unique</span></li>
        </ul>
        <FieldHelp
          examples={[
            { input: '{datetime}-{random}', output: '2026-05-31T143015-a1b2' },
            { input: '{hostname}-{date}', output: 'her-pc-2026-05-31' },
          ]}
        />

        <div class="field">
          <label for="archive-template">Template</label>
          <input id="archive-template" type="text" bind:value={archiveTemplate} placeholder="{'{datetime}-{random}'}" />
        </div>

        <div class="field">
          <span class="field-label">Preview</span>
          <code class="preview-box">{archivePreview || '...'}</code>
        </div>

        <div class="form-actions">
          <button type="submit" class="btn btn-primary" disabled={archiveTemplateSaving}>
            {archiveTemplateSaving ? 'Saving...' : 'Save'}
          </button>
        </div>

        {#if archiveTemplateResult}
          <div class="test-result" class:success={archiveTemplateResult.includes('saved') || archiveTemplateResult.includes('reset')} class:error={archiveTemplateResult.includes('Failed') || archiveTemplateResult.includes('No active')}>
            {archiveTemplateResult}
          </div>
        {/if}
      </fieldset>
    </form>
  {/if}

  <form class="settings-form" onsubmit={(e) => { e.preventDefault(); save(); }}>
    <fieldset class="form-group">
      <legend>Connection</legend>
      <FieldHelp text="Where should your backups be stored? Pick the kind of destination, then fill in the details below." />

      <div class="repo-type-toggle" role="radiogroup" aria-label="Repository type">
        <button
          type="button"
          class="repo-type-option"
          class:active={repoType === 'ssh'}
          role="radio"
          aria-checked={repoType === 'ssh'}
          onclick={() => (repoType = 'ssh')}
        >
          <span class="repo-type-title">Backup server (SSH)</span>
          <span class="repo-type-sub">A remote server you connect to over the internet.</span>
        </button>
        <button
          type="button"
          class="repo-type-option"
          class:active={repoType === 'local'}
          role="radio"
          aria-checked={repoType === 'local'}
          onclick={() => (repoType = 'local')}
        >
          <span class="repo-type-title">Local folder / USB / network drive</span>
          <span class="repo-type-sub">A folder on this PC, an external/USB drive, or a network share. No server needed.</span>
        </button>
      </div>

      {#if repoType === 'local'}
        <div class="field">
          <label for="local-path">Backup folder path</label>
          <div class="inline-row">
            <input id="local-path" type="text" bind:value={repoPath} placeholder="E:\Backups\her-pc" />
            <button type="button" class="btn btn-secondary" onclick={browseLocalRepoFolder}>Browse…</button>
          </div>
          <FieldHelp
            text="Back up to a folder on this PC, an external/USB drive, or a network share. Pick or type the folder where the backup should live."
            examples={[
              { input: 'E:\\Backups\\her-pc' },
              { input: '\\\\nas\\backups\\her-pc' },
            ]}
          />
        </div>

        <div class="form-actions">
          <button type="submit" class="btn btn-primary" disabled={saving || !repoConfigured}>
            {saving ? 'Saving...' : 'Save'}
          </button>
        </div>
      {:else}
        <div class="field">
          <label for="ssh-host">Host</label>
          <input id="ssh-host" type="text" bind:value={sshHost} placeholder="backup.example.com" />
          <FieldHelp text="The address of your backup server." examples={[{ input: 'backup.example.com' }]} />
        </div>

        <div class="field-row">
          <div class="field">
            <label for="ssh-user">User</label>
            <input id="ssh-user" type="text" bind:value={sshUser} placeholder="borg" />
          </div>
          <div class="field field-sm">
            <label for="ssh-port">Port</label>
            <input id="ssh-port" type="number" bind:value={sshPort} />
          </div>
        </div>
        <FieldHelp text="The login name on the server, and the SSH port (almost always 22)." examples={[{ input: 'user borg' }, { input: 'port 22' }]} />

        <div class="field">
          <label for="repo-path">Repository Path</label>
          <input id="repo-path" type="text" bind:value={repoPath} placeholder="/backups/her-pc" />
          <FieldHelp text="The folder on the server where this PC's backups are kept." examples={[{ input: '/backups/her-pc' }]} />
        </div>

        <div class="field">
          <label for="ssh-key">SSH Key Path (optional)</label>
          <input id="ssh-key" type="text" bind:value={sshKeyPath} placeholder="C:\Users\her\.ssh\id_ed25519" />
          <FieldHelp text="A key file that lets you log in without typing the server password each time. Leave blank if you don't use one." examples={[{ input: 'C:\\Users\\her\\.ssh\\id_ed25519' }]} />
        </div>

        <div class="form-actions">
          <button type="button" class="btn btn-secondary" onclick={testConnection} disabled={testing || !sshHost}>
            {testing ? 'Testing...' : 'Test Connection'}
          </button>
          <button type="submit" class="btn btn-primary" disabled={saving || !sshHost || !repoPath}>
            {saving ? 'Saving...' : 'Save'}
          </button>
        </div>

        {#if testResult}
          <div class="test-result" class:success={testResult.includes('successful')} class:error={testResult.includes('Error') || testResult.includes('failed')}>
            {testResult}
          </div>
        {/if}
      {/if}

      {#if saveResult}
        <div class="test-result" class:success={saveResult === 'Settings saved.'} class:error={saveResult.includes('failed')}>
          {saveResult}
        </div>
      {/if}
    </fieldset>
  </form>

  <form class="settings-form" onsubmit={(e) => { e.preventDefault(); initRepo(); }}>
    <fieldset class="form-group">
      <legend>Initialize Repository</legend>
      <FieldHelp text="“Initialize” sets up a fresh, empty backup repository at the destination above. Do this once for a brand-new destination. Skip it if you're connecting to a backup that already exists — initializing again is not needed." />

      <div class="field">
        <label for="init-encryption">Encryption</label>
        <select id="init-encryption" bind:value={initEncryption}>
          <option value="repokey-blake2">repokey-blake2 (recommended)</option>
          <option value="repokey">repokey</option>
          <option value="keyfile-blake2">keyfile-blake2</option>
          <option value="keyfile">keyfile</option>
          <option value="authenticated-blake2">authenticated-blake2 (no encryption)</option>
          <option value="authenticated">authenticated (no encryption)</option>
          <option value="none">none (no encryption, no auth)</option>
        </select>
        <ul class="var-help">
          <li><code>repokey-blake2</code> <span>recommended — encrypts your files; the key lives inside the repository</span></li>
          <li><code>repokey</code> <span>same idea, slightly slower checksum</span></li>
          <li><code>keyfile-blake2</code> <span>encrypts your files; key stored on this PC (back it up!)</span></li>
          <li><code>keyfile</code> <span>same, slightly slower checksum</span></li>
          <li><code>authenticated</code> <span>tamper-detection only</span></li>
          <li><code>none</code> <span>no protection at all</span></li>
        </ul>
      </div>

      <div class="warning-box">
        <strong>Heads up:</strong> <code>none</code>, <code>authenticated</code> and
        <code>authenticated-blake2</code> do <strong>not</strong> encrypt your file contents.
        Anyone with access to the backup could read your files. For private data, keep the
        recommended <code>repokey-blake2</code>.
      </div>

      {#if needsPassphrase}
        <div class="field">
          <label for="init-passphrase">Passphrase</label>
          <input id="init-passphrase" type="password" bind:value={initPassphrase} autocomplete="new-password" />
        </div>
        <div class="field">
          <label for="init-passphrase-confirm">Confirm passphrase</label>
          <input id="init-passphrase-confirm" type="password" bind:value={initPassphraseConfirm} autocomplete="new-password" />
        </div>
        <div class="warning-box">
          Write down your passphrase and keep it somewhere safe. Without it, your backups
          can never be restored — not by you, not by anyone.
        </div>
      {/if}

      <div class="form-actions">
        <button type="submit" class="btn btn-primary" disabled={initing || !repoConfigured}>
          {initing ? 'Initializing...' : 'Create Repository'}
        </button>
      </div>

      {#if initResult}
        <div class="test-result" class:success={initResult.includes('success')} class:error={initResult.includes('failed') || initResult.includes('required') || initResult.includes('do not match')}>
          {initResult}
        </div>
      {/if}
    </fieldset>
  </form>

  <form class="settings-form" onsubmit={(e) => e.preventDefault()}>
    <fieldset class="form-group">
      <legend>Repository Passphrase</legend>
      <FieldHelp text="This is the passphrase that encrypts the repository itself — the one you chose when you initialized it. It is NOT the SSH key password. It's saved securely in Windows Credential Manager and used automatically for every backup and restore, so you won't be asked for it each time." />

      <div class="passphrase-status">
        <span class="status-dot" class:set={hasPassphrase}></span>
        <span>
          {#if passphraseLoading}
            Checking…
          {:else if hasPassphrase}
            Passphrase is set for this repository
          {:else}
            No passphrase stored
          {/if}
        </span>
      </div>

      <div class="form-actions">
        <button type="button" class="btn btn-primary" onclick={openPassphraseModal} disabled={!repoConfigured}>
          {hasPassphrase ? 'Change passphrase' : 'Set passphrase'}
        </button>
        {#if hasPassphrase}
          <button type="button" class="btn btn-secondary" onclick={() => (clearPassphraseModalOpen = true)}>
            Clear
          </button>
        {/if}
      </div>

      {#if passphraseResult && !passphraseModalOpen}
        <div class="test-result" class:success={passphraseResult.includes('saved') || passphraseResult.includes('removed')} class:error={passphraseResult.includes('Failed') || passphraseResult.includes('first') || passphraseResult.includes('match') || passphraseResult.includes('empty')}>
          {passphraseResult}
        </div>
      {/if}
    </fieldset>
  </form>

  <form class="settings-form" onsubmit={(e) => { e.preventDefault(); saveRetention(); }}>
    <fieldset class="form-group">
      <legend>Retention Policy</legend>
      <FieldHelp text="Old backups add up over time. This policy thins them out automatically: it keeps a recent few of each kind and deletes (prunes) the older ones that fall outside the rules. Leave a box empty to keep unlimited backups for that period." />
      <FieldHelp text="A common, comfortable setup: keep 7 daily + 4 weekly + 6 monthly. That's roughly six months of history, automatically thinned so it never balloons." />
      <FieldHelp text='"Run Prune Now" applies these rules immediately. Saving the policy makes scheduled backups apply it for you.' />

      <div class="field-row">
        <div class="field field-sm">
          <label for="keep-hourly">Keep hourly</label>
          <input id="keep-hourly" type="number" min="0" placeholder="—" bind:value={keepHourly} />
        </div>
        <div class="field field-sm">
          <label for="keep-daily">Keep daily</label>
          <input id="keep-daily" type="number" min="0" placeholder="—" bind:value={keepDaily} />
        </div>
        <div class="field field-sm">
          <label for="keep-weekly">Keep weekly</label>
          <input id="keep-weekly" type="number" min="0" placeholder="—" bind:value={keepWeekly} />
        </div>
      </div>
      <div class="field-row">
        <div class="field field-sm">
          <label for="keep-monthly">Keep monthly</label>
          <input id="keep-monthly" type="number" min="0" placeholder="—" bind:value={keepMonthly} />
        </div>
        <div class="field field-sm">
          <label for="keep-yearly">Keep yearly</label>
          <input id="keep-yearly" type="number" min="0" placeholder="—" bind:value={keepYearly} />
        </div>
      </div>

      <div class="form-actions">
        <button type="submit" class="btn btn-secondary" disabled={retentionSaving || retentionPruning}>
          {retentionSaving ? 'Saving...' : 'Save Policy'}
        </button>
        <button type="button" class="btn btn-primary" onclick={runPrune} disabled={retentionPruning || retentionSaving || !repoConfigured}>
          {retentionPruning ? 'Pruning...' : 'Run Prune Now'}
        </button>
      </div>

      {#if retentionResult}
        <div class="test-result" class:success={retentionResult.includes('success') || retentionResult.includes('saved')} class:error={retentionResult.includes('failed')}>
          {retentionResult}
        </div>
      {/if}
    </fieldset>
  </form>

  <form class="settings-form" onsubmit={(e) => e.preventDefault()}>
    <fieldset class="form-group">
      <legend>Notifications</legend>
      <FieldHelp text="Show a desktop notification when a backup or restore finishes or fails. The first time you turn this on, Windows will ask for permission to show notifications — choose Allow. If you miss that prompt, you can enable notifications for BorgUI later in Windows Settings." />

      <div class="field">
        <label class="toggle-row">
          <input type="checkbox" checked={notificationsEnabled} onchange={(e) => toggleNotifications(e.currentTarget.checked)} />
          <span>Enable desktop notifications</span>
        </label>
      </div>

      {#if notificationsResult}
        <div class="test-result" class:success={notificationsResult.includes('enabled') || notificationsResult.includes('disabled')} class:error={notificationsResult.includes('denied') || notificationsResult.includes('Error')}>
          {notificationsResult}
        </div>
      {/if}
    </fieldset>
  </form>

  <form class="settings-form" onsubmit={(e) => { e.preventDefault(); saveSchedule(); }}>
    <fieldset class="form-group">
      <legend>Scheduled Backups</legend>
      <FieldHelp text="Let BorgUI back up on its own using Windows Task Scheduler. Because Windows runs it, scheduled backups happen even when this app is closed — you just need BorgUI installed on the PC, not open. Choose “Every hour” for frequent protection, or “Daily” to run once at a set time (a quiet hour like 2:00 AM is a good choice)." />

      <div class="field">
        <label class="toggle-row">
          <input type="checkbox" bind:checked={scheduleEnabled} />
          <span>Enable scheduled backups</span>
        </label>
      </div>

      {#if scheduleEnabled}
        <div class="field">
          <label for="schedule-type">Frequency</label>
          <select id="schedule-type" bind:value={scheduleType}>
            <option value="hourly">Every hour</option>
            <option value="daily">Daily</option>
          </select>
        </div>

        {#if scheduleNextRunLabel}
          <div class="next-run">
            <span class="next-run-label">Next run</span>
            <span class="next-run-value">{scheduleNextRunLabel}</span>
          </div>
        {/if}

        {#if scheduleType === 'daily'}
          <div class="field-row">
            <div class="field field-sm">
              <label for="schedule-hour">Hour</label>
              <input id="schedule-hour" type="number" min="0" max="23" bind:value={scheduleHour} />
            </div>
            <div class="field field-sm">
              <label for="schedule-minute">Minute</label>
              <input id="schedule-minute" type="number" min="0" max="59" bind:value={scheduleMinute} />
            </div>
          </div>
        {/if}

        <div class="field">
          <span class="field-label">Source Folders</span>
          <div class="path-list">
            {#if schedulePaths.length === 0}
              <p class="empty-hint">No folders selected</p>
            {/if}
            {#each schedulePaths as path, i}
              <div class="path-item">
                <code>{path}</code>
                <button type="button" onclick={() => schedulePaths = schedulePaths.filter((_, idx) => idx !== i)}>✕</button>
              </div>
            {/each}
          </div>
          <button type="button" class="btn btn-secondary" onclick={addScheduleFolder}>
            + Add Folder
          </button>
        </div>

        <div class="field">
          <span class="field-label">Exclude Patterns</span>
          {#if scheduleExcludes.length > 0}
            <div class="chip-list">
              {#each scheduleExcludes as pattern, i}
                <span class="chip">
                  <code>{pattern}</code>
                  <button type="button" class="chip-remove" onclick={() => removeScheduleExclude(i)} aria-label="Remove pattern">✕</button>
                </span>
              {/each}
            </div>
          {/if}
          <div class="exclude-input-row">
            <input
              type="text"
              class="exclude-input"
              placeholder="e.g. *.log or node_modules"
              bind:value={scheduleExcludeInput}
              onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); addScheduleExclude(scheduleExcludeInput); } }}
            />
            <button type="button" class="btn btn-secondary" onclick={() => addScheduleExclude(scheduleExcludeInput)} disabled={!scheduleExcludeInput.trim()}>
              + Add
            </button>
          </div>
          <div class="preset-row">
            <span class="preset-label">Presets:</span>
            {#each EXCLUDE_PRESETS as preset}
              <button
                type="button"
                class="preset-chip"
                onclick={() => addScheduleExclude(preset)}
                disabled={scheduleExcludes.includes(preset)}
              >
                {preset}
              </button>
            {/each}
          </div>
        </div>
      {/if}

      <div class="form-actions">
        <button type="submit" class="btn btn-primary" disabled={scheduleSaving}>
          {scheduleSaving ? 'Saving...' : 'Save Schedule'}
        </button>
      </div>

      {#if scheduleResult}
        <div class="test-result" class:success={scheduleResult.includes('saved') || scheduleResult.includes('disabled')} class:error={scheduleResult.includes('failed')}>
          {scheduleResult}
        </div>
      {/if}
    </fieldset>
  </form>

  {#if renameModalOpen}
    <div class="modal-backdrop" onclick={() => (renameModalOpen = false)} role="presentation">
      <div
        class="modal"
        onclick={(e) => e.stopPropagation()}
        onkeydown={() => {}}
        role="dialog"
        tabindex="-1"
        aria-modal="true"
        aria-labelledby="rename-title"
      >
        <h2 id="rename-title">Rename profile</h2>
        <form onsubmit={(e) => { e.preventDefault(); confirmRename(); }}>
          <div class="field">
            <label for="rename-input">Profile name</label>
            <!-- svelte-ignore a11y_autofocus -->
            <input id="rename-input" type="text" bind:value={renameInput} autofocus />
          </div>
          <div class="modal-actions">
            <button type="button" class="btn btn-secondary" onclick={() => (renameModalOpen = false)}>Cancel</button>
            <button type="submit" class="btn btn-primary" disabled={!renameInput.trim()}>Save</button>
          </div>
        </form>
      </div>
    </div>
  {/if}

  {#if deleteModalOpen}
    <div class="modal-backdrop" onclick={() => (deleteModalOpen = false)} role="presentation">
      <div
        class="modal"
        onclick={(e) => e.stopPropagation()}
        onkeydown={() => {}}
        role="dialog"
        tabindex="-1"
        aria-modal="true"
        aria-labelledby="delete-profile-title"
      >
        <h2 id="delete-profile-title">Delete profile?</h2>
        <p>This removes the profile <code>{profilesState.active?.name}</code> and its settings (connection, schedule, retention). Your backups in the repository are NOT deleted.</p>
        <div class="modal-actions">
          <button type="button" class="btn btn-secondary" onclick={() => (deleteModalOpen = false)}>Cancel</button>
          <button type="button" class="btn btn-delete-confirm" onclick={confirmDeleteProfile}>Delete</button>
        </div>
      </div>
    </div>
  {/if}

  {#if clearPassphraseModalOpen}
    <div class="modal-backdrop" onclick={() => (clearPassphraseModalOpen = false)} role="presentation">
      <div
        class="modal"
        onclick={(e) => e.stopPropagation()}
        onkeydown={() => {}}
        role="dialog"
        tabindex="-1"
        aria-modal="true"
        aria-labelledby="clear-pass-title"
      >
        <h2 id="clear-pass-title">Remove passphrase?</h2>
        <p>This removes the saved passphrase from Windows Credential Manager. Backups and restores will fail until you set it again. Your existing backups are not affected.</p>
        <div class="modal-actions">
          <button type="button" class="btn btn-secondary" onclick={() => (clearPassphraseModalOpen = false)}>Cancel</button>
          <button type="button" class="btn btn-delete-confirm" onclick={confirmClearPassphrase}>Remove</button>
        </div>
      </div>
    </div>
  {/if}

  {#if passphraseModalOpen}
    <div class="modal-backdrop" onclick={() => (passphraseModalOpen = false)} role="presentation">
      <div
        class="modal"
        onclick={(e) => e.stopPropagation()}
        onkeydown={() => {}}
        role="dialog"
        tabindex="-1"
        aria-modal="true"
        aria-labelledby="passphrase-title"
      >
        <h2 id="passphrase-title">{hasPassphrase ? 'Change passphrase' : 'Set passphrase'}</h2>
        <p>Enter the passphrase used to encrypt this borg repository. It will be stored in your OS keychain.</p>
        <form onsubmit={(e) => { e.preventDefault(); savePassphrase(); }}>
          <div class="field">
            <label for="pass-input">Passphrase</label>
            <input id="pass-input" type="password" autocomplete="new-password" bind:value={passphraseInput} />
          </div>
          <div class="field">
            <label for="pass-confirm">Confirm</label>
            <input id="pass-confirm" type="password" autocomplete="new-password" bind:value={passphraseConfirm} />
          </div>
          {#if passphraseResult}
            <div class="test-result error">{passphraseResult}</div>
          {/if}
          <div class="modal-actions">
            <button type="button" class="btn btn-secondary" onclick={() => (passphraseModalOpen = false)}>Cancel</button>
            <button type="submit" class="btn btn-primary" disabled={passphraseSaving}>
              {passphraseSaving ? 'Saving…' : 'Save'}
            </button>
          </div>
        </form>
      </div>
    </div>
  {/if}
</div>

<style>
  .settings-page {
    max-width: 560px;
  }

  .page-header {
    margin-bottom: var(--space-8);
  }

  .page-header h1 {
    font-size: var(--text-3xl);
    font-weight: 700;
    letter-spacing: -0.03em;
  }

  .subtitle {
    color: var(--color-text-muted);
    margin-top: var(--space-1);
  }

  .settings-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }

  .form-group {
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-lg);
    padding: var(--space-6);
    background: var(--color-surface);
  }

  .form-group legend {
    font-size: var(--text-sm);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--color-text-muted);
    padding: 0 var(--space-2);
  }

  .profile-current {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
    margin-top: var(--space-3);
    padding: var(--space-3);
    background: var(--color-bg);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
  }

  .profile-current code {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--color-text);
    flex: 1;
  }

  .profile-actions {
    display: flex;
    gap: var(--space-2);
  }

  .inline-row {
    display: flex;
    gap: var(--space-2);
  }

  .inline-row input {
    flex: 1;
  }

  .preview-box {
    display: block;
    background: var(--color-bg);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-sm);
    padding: var(--space-2) var(--space-3);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--color-accent);
    word-break: break-all;
  }

  .settings-form + .settings-form {
    margin-top: var(--space-6);
  }

  .var-help {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin-top: var(--space-3);
    padding: var(--space-2) var(--space-3);
    background: var(--color-bg);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-sm);
  }

  .var-help li {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-size: var(--text-xs);
  }

  .var-help code {
    font-family: var(--font-mono);
    font-size: 0.7rem;
    color: var(--color-accent);
    flex-shrink: 0;
    min-width: 7.5rem;
  }

  .var-help span {
    color: var(--color-text-dim);
    line-height: 1.4;
  }

  .warning-box {
    margin-top: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius-sm);
    background: var(--color-warning-muted);
    border: 1px solid var(--color-warning);
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    line-height: 1.5;
  }

  .warning-box strong {
    color: var(--color-warning);
  }

  .warning-box code {
    font-family: var(--font-mono);
    font-size: 0.7rem;
    color: var(--color-warning);
  }

  .repo-type-toggle {
    display: flex;
    gap: var(--space-3);
    margin-top: var(--space-4);
  }

  .repo-type-option {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    text-align: left;
    padding: var(--space-3) var(--space-4);
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    cursor: pointer;
    transition: all var(--duration-fast) var(--ease-out);
  }

  .repo-type-option:hover {
    border-color: var(--color-text-muted);
  }

  .repo-type-option.active {
    border-color: var(--color-accent);
    background: var(--color-accent-muted);
  }

  .repo-type-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .repo-type-option.active .repo-type-title {
    color: var(--color-accent);
  }

  .repo-type-sub {
    font-size: var(--text-xs);
    color: var(--color-text-dim);
    line-height: 1.4;
  }

  .next-run {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    margin-top: var(--space-4);
    padding: var(--space-2) var(--space-3);
    background: var(--color-bg);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-sm);
  }

  .next-run-label {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--color-text-dim);
    font-weight: 600;
  }

  .next-run-value {
    font-size: var(--text-sm);
    font-family: var(--font-mono);
    color: var(--color-accent);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin-top: var(--space-4);
  }

  .field label,
  .field .field-label {
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-text-muted);
  }

  .field input {
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: var(--space-2) var(--space-3);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    transition: border-color var(--duration-fast) var(--ease-out);
  }

  .field input:focus {
    outline: none;
    border-color: var(--color-accent);
  }

  .field input::placeholder {
    color: var(--color-text-dim);
  }

  .field-row {
    display: flex;
    gap: var(--space-4);
  }

  .field-row .field {
    flex: 1;
  }

  .field-row .field-sm {
    flex: 0 0 100px;
  }

  .form-actions {
    display: flex;
    gap: var(--space-3);
    margin-top: var(--space-6);
  }

  .btn {
    padding: var(--space-2) var(--space-4);
    border-radius: var(--radius-md);
    font-weight: 500;
    font-size: var(--text-sm);
    transition: all var(--duration-fast) var(--ease-out);
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-primary {
    background: var(--color-accent);
    color: var(--color-on-accent);
  }

  .btn-primary:hover:not(:disabled) {
    background: var(--color-accent-hover);
  }

  .btn-secondary {
    background: var(--color-surface-hover);
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
  }

  .btn-secondary:hover:not(:disabled) {
    background: var(--color-surface-active);
    color: var(--color-text);
  }

  .test-result {
    margin-top: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
    background: var(--color-surface-hover);
    color: var(--color-text-muted);
  }

  .test-result.success {
    background: var(--color-success-muted);
    color: var(--color-success);
  }

  .test-result.error {
    background: var(--color-danger-muted);
    color: var(--color-danger);
  }

  .toggle-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    cursor: pointer;
    font-size: var(--text-sm);
    color: var(--color-text);
  }

  .toggle-row input[type="checkbox"] {
    width: 16px;
    height: 16px;
    accent-color: var(--color-accent);
  }

  select {
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: var(--space-2) var(--space-3);
    color: var(--color-text);
    font-size: var(--text-sm);
  }

  select:focus {
    outline: none;
    border-color: var(--color-accent);
  }

  .path-list {
    background: var(--color-bg);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    padding: var(--space-3);
    min-height: 60px;
  }

  .empty-hint {
    color: var(--color-text-dim);
    text-align: center;
    padding: var(--space-3);
    font-size: var(--text-sm);
  }

  .path-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-hover);
    border-radius: var(--radius-sm);
  }

  .path-item + .path-item {
    margin-top: var(--space-2);
  }

  .path-item code {
    font-size: var(--text-sm);
  }

  .path-item button {
    color: var(--color-text-dim);
    padding: var(--space-1);
  }

  .path-item button:hover {
    color: var(--color-danger);
  }

  .chip-list {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--color-surface-hover);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    padding: var(--space-1) var(--space-3);
    font-size: var(--text-sm);
  }

  .chip code {
    font-family: var(--font-mono);
  }

  .chip-remove {
    background: transparent;
    border: none;
    color: var(--color-text-dim);
    cursor: pointer;
    padding: 0 2px;
    font-size: var(--text-xs);
  }

  .chip-remove:hover:not(:disabled) {
    color: var(--color-danger);
  }

  .exclude-input-row {
    display: flex;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
  }

  .exclude-input {
    flex: 1;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-3);
    color: var(--color-text);
    font-size: var(--text-sm);
    font-family: var(--font-mono);
  }

  .exclude-input:focus {
    outline: none;
    border-color: var(--color-accent);
  }

  .preset-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }

  .preset-label {
    font-size: var(--text-xs);
    color: var(--color-text-dim);
    margin-right: var(--space-1);
  }

  .preset-chip {
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: 2px var(--space-2);
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    cursor: pointer;
    transition: all var(--duration-fast) var(--ease-out);
  }

  .preset-chip:hover:not(:disabled) {
    background: var(--color-surface-hover);
    color: var(--color-text);
    border-color: var(--color-text-muted);
  }

  .preset-chip:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .passphrase-status {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin-top: var(--space-3);
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--color-text-dim);
  }

  .status-dot.set {
    background: var(--color-success);
  }

  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: var(--color-backdrop);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .modal {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-6);
    max-width: 440px;
    width: 90%;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .modal h2 {
    font-size: var(--text-lg);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .modal p {
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    line-height: 1.5;
  }

  .modal code {
    font-family: var(--font-mono);
    color: var(--color-text);
    background: var(--color-surface-hover);
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
  }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-4);
  }
</style>
