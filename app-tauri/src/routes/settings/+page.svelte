<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open, save as saveDialog } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import { repoState, type RepoConfig } from '$lib/stores/repo.svelte';
  import { scheduleState, type ScheduleConfig } from '$lib/stores/schedule.svelte';
  import { retentionState, type RetentionConfig } from '$lib/stores/retention.svelte';
  import { notificationsState } from '$lib/stores/notifications.svelte';
  import { profilesState } from '$lib/stores/profiles.svelte';

  let sshHost = $state('');
  let sshPort = $state(22);
  let sshUser = $state('');
  let repoPath = $state('');
  let sshKeyPath = $state('');
  let testing = $state(false);
  let saving = $state(false);
  let testResult = $state('');
  let saveResult = $state('');

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
      profileResult = 'Fill in SSH host, user, and repo path first';
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

  async function renameActive() {
    const id = profilesState.activeId;
    if (!id) return;
    const newName = prompt('New profile name', profilesState.active?.name ?? '');
    if (!newName) return;
    try {
      await profilesState.rename(id, newName);
      profileResult = `Renamed to "${newName}"`;
    } catch (e) {
      profileResult = `Failed: ${e}`;
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

  async function deleteActive() {
    const id = profilesState.activeId;
    if (!id) return;
    if (profilesState.profiles.length <= 1) {
      profileResult = 'Cannot delete the only profile';
      return;
    }
    if (!confirm(`Delete profile "${profilesState.active?.name}"? Repo config is removed; archives are not touched.`)) return;
    try {
      await profilesState.remove(id);
      await repoState.load();
      profileResult = 'Profile deleted';
    } catch (e) {
      profileResult = `Failed: ${e}`;
    }
  }

  // Local mirror of the store value so the checkbox reflects the user's
  // *attempt* even when the OS rejects permission, then we roll it back.
  let notificationsEnabled = $state(notificationsState.enabled);
  let notificationsResult = $state('');

  function currentRepoFromForm(): RepoConfig | null {
    if (!sshHost || !repoPath || !sshUser) return null;
    return {
      ssh_host: sshHost,
      ssh_port: sshPort,
      ssh_user: sshUser,
      repo_path: repoPath,
      ssh_key_path: sshKeyPath || null,
    };
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

  async function clearPassphrase() {
    const repo = currentRepoFromForm();
    if (!repo) return;
    if (!confirm('Remove the passphrase from the system keychain? Backups will fail until you set it again.')) return;
    try {
      await invoke('clear_repo_passphrase', { repo });
      hasPassphrase = false;
      passphraseResult = 'Passphrase removed from keychain.';
    } catch (e) {
      passphraseResult = `Failed to clear passphrase: ${e}`;
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
      sshHost = r.ssh_host;
      sshPort = r.ssh_port;
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
      const repo: RepoConfig = {
        ssh_host: sshHost,
        ssh_port: sshPort,
        ssh_user: sshUser,
        repo_path: repoPath,
        ssh_key_path: sshKeyPath || null,
      };
      await repoState.save(repo);
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
      const repo: RepoConfig = {
        ssh_host: sshHost,
        ssh_port: sshPort,
        ssh_user: sshUser,
        repo_path: repoPath,
        ssh_key_path: sshKeyPath || null,
      };
      await invoke('init_repo', {
        repo,
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
      const repo: RepoConfig = {
        ssh_host: sshHost,
        ssh_port: sshPort,
        ssh_user: sshUser,
        repo_path: repoPath,
        ssh_key_path: sshKeyPath || null,
      };
      await invoke('prune_repo', { repo, retention: currentRetention() });
      retentionResult = 'Prune completed successfully.';
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
      <p class="hint">A profile bundles a repository plus its schedule and retention settings. Switch profiles via the picker in the sidebar.</p>

      {#if profilesState.active}
        <div class="profile-current">
          <span class="field-label">Active</span>
          <code>{profilesState.active.name}</code>
          <div class="profile-actions">
            <button type="button" class="btn btn-secondary" onclick={renameActive}>Rename</button>
            <button type="button" class="btn btn-secondary" onclick={exportActive}>Export</button>
            <button type="button" class="btn btn-secondary" onclick={deleteActive} disabled={profilesState.profiles.length <= 1}>Delete</button>
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
        <p class="hint">Template for new archive names. Variables: <code>{'{date}'}</code>, <code>{'{time}'}</code>, <code>{'{datetime}'}</code>, <code>{'{hostname}'}</code>, <code>{'{profile}'}</code>, <code>{'{random}'}</code>. Leave blank to use the default <code>{'{datetime}-{random}'}</code>.</p>

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
      <legend>SSH Connection</legend>

      <div class="field">
        <label for="ssh-host">Host</label>
        <input id="ssh-host" type="text" bind:value={sshHost} placeholder="backup.example.com" />
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

      <div class="field">
        <label for="repo-path">Repository Path</label>
        <input id="repo-path" type="text" bind:value={repoPath} placeholder="/data/backups/my-pc" />
      </div>

      <div class="field">
        <label for="ssh-key">SSH Key Path (optional)</label>
        <input id="ssh-key" type="text" bind:value={sshKeyPath} placeholder="C:\Users\you\.ssh\id_ed25519" />
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
      <p class="hint">Create a new borg repository at the configured path. Skip if you're connecting to an existing repo.</p>

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
      {/if}

      <div class="form-actions">
        <button type="submit" class="btn btn-primary" disabled={initing || !sshHost || !repoPath}>
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
      <p class="hint">Stored in your OS keychain (Windows Credential Manager, macOS Keychain, or Secret Service). Used automatically for borg commands that need it.</p>

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
        <button type="button" class="btn btn-primary" onclick={openPassphraseModal} disabled={!sshHost || !repoPath}>
          {hasPassphrase ? 'Change passphrase' : 'Set passphrase'}
        </button>
        {#if hasPassphrase}
          <button type="button" class="btn btn-secondary" onclick={clearPassphrase}>
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
      <p class="hint">Set how many backups to keep per time bucket. Empty = no limit for that bucket. Pruning removes archives that fall outside the policy.</p>

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
        <button type="button" class="btn btn-primary" onclick={runPrune} disabled={retentionPruning || retentionSaving || !sshHost || !repoPath}>
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
      <p class="hint">Show a desktop notification when a backup or restore completes or fails.</p>

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

  .form-group .hint {
    margin-top: var(--space-3);
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    line-height: 1.5;
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
    color: oklch(14% 0 0);
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
    background: oklch(72% 0.16 145 / 0.15);
    color: var(--color-success);
  }

  .test-result.error {
    background: oklch(65% 0.2 25 / 0.15);
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
    background: oklch(0% 0 0 / 0.5);
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

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-4);
  }
</style>
