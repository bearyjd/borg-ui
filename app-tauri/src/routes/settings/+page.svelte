<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { repoConfig, saveRepoConfig, type RepoConfig } from '$lib/stores/repo';

  let sshHost = $state('');
  let sshPort = $state(22);
  let sshUser = $state('');
  let repoPath = $state('');
  let sshKeyPath = $state('');
  let testing = $state(false);
  let saving = $state(false);
  let testResult = $state('');
  let saveResult = $state('');

  onMount(() => {
    const unsub = repoConfig.subscribe((r) => {
      if (r) {
        sshHost = r.ssh_host;
        sshPort = r.ssh_port;
        sshUser = r.ssh_user;
        repoPath = r.repo_path;
        sshKeyPath = r.ssh_key_path ?? '';
      }
    });
    return unsub;
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
      await saveRepoConfig(repo);
      saveResult = 'Settings saved.';
    } catch (e) {
      saveResult = `Save failed: ${e}`;
    } finally {
      saving = false;
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

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin-top: var(--space-4);
  }

  .field label {
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
</style>
