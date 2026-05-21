<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import { repoState, type RepoConfig } from '$lib/stores/repo.svelte';
  import { scheduleState, type ScheduleConfig } from '$lib/stores/schedule.svelte';

  let sshHost = $state('');
  let sshPort = $state(22);
  let sshUser = $state('');
  let repoPath = $state('');
  let sshKeyPath = $state('');
  let testing = $state(false);
  let saving = $state(false);
  let testResult = $state('');
  let saveResult = $state('');

  let scheduleEnabled = $state(false);
  let scheduleType = $state<'hourly' | 'daily'>('daily');
  let scheduleHour = $state(2);
  let scheduleMinute = $state(0);
  let schedulePaths = $state<string[]>([]);
  let scheduleSaving = $state(false);
  let scheduleResult = $state('');

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
  });

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
</style>
