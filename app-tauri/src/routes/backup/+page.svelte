<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import { repoConfig, hasRepo, type RepoConfig } from '$lib/stores/repo';

  let sourcePaths = $state<string[]>([]);
  let isRunning = $state(false);
  let status = $state('');
  let repo = $state<RepoConfig | null>(null);
  let repoAvailable = $state(false);

  $effect(() => {
    const unsub1 = repoConfig.subscribe((r) => (repo = r));
    const unsub2 = hasRepo.subscribe((v) => (repoAvailable = v));
    return () => { unsub1(); unsub2(); };
  });

  async function addFolder() {
    const selected = await open({ directory: true, multiple: false, title: 'Select folder to back up' });
    if (selected) {
      sourcePaths = [...sourcePaths, selected as string];
    }
  }

  async function runBackup() {
    if (!repo) {
      status = 'No repository configured. Go to Settings first.';
      return;
    }
    if (sourcePaths.length === 0) {
      status = 'Please add at least one folder to back up.';
      return;
    }

    isRunning = true;
    status = 'Starting backup...';

    try {
      const archiveName = `backup-${new Date().toISOString().replace(/[:.]/g, '-')}`;
      await invoke('create_backup', {
        repo,
        sourcePaths,
        archiveName,
      });
      status = 'Backup completed successfully!';
    } catch (e) {
      status = `Backup failed: ${e}`;
    } finally {
      isRunning = false;
    }
  }
</script>

<div class="backup-page">
  <header class="page-header">
    <h1>Backup</h1>
    <p class="subtitle">Create a new backup archive</p>
  </header>

  {#if !repoAvailable}
    <div class="warning-banner">
      No repository configured. <a href="/settings">Set up your connection</a> first.
    </div>
  {/if}

  <div class="backup-form">
    <div class="form-section">
      <span class="form-label">Source Folders</span>
      <div class="path-list">
        {#if sourcePaths.length === 0}
          <p class="empty-hint">No folders selected</p>
        {/if}
        {#each sourcePaths as path, i}
          <div class="path-item">
            <code>{path}</code>
            <button onclick={() => sourcePaths = sourcePaths.filter((_, idx) => idx !== i)}>✕</button>
          </div>
        {/each}
      </div>
      <button class="btn btn-secondary" onclick={addFolder} disabled={isRunning}>
        + Add Folder
      </button>
    </div>

    <div class="form-actions">
      <button class="btn btn-primary" onclick={runBackup} disabled={isRunning || !repoAvailable}>
        {isRunning ? 'Backing up...' : 'Start Backup'}
      </button>
    </div>

    {#if status}
      <div class="status-message" class:error={status.includes('failed') || status.includes('No repository')}>
        {status}
      </div>
    {/if}
  </div>
</div>

<style>
  .backup-page {
    max-width: 640px;
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

  .warning-banner {
    background: oklch(78% 0.16 80 / 0.15);
    color: var(--color-warning);
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius-md);
    font-size: var(--text-sm);
    margin-bottom: var(--space-6);
  }

  .warning-banner a {
    color: var(--color-warning);
    text-decoration: underline;
  }

  .backup-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }

  .form-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .form-label {
    font-size: var(--text-sm);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--color-text-muted);
  }

  .path-list {
    background: var(--color-surface);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    padding: var(--space-3);
    min-height: 80px;
  }

  .empty-hint {
    color: var(--color-text-dim);
    text-align: center;
    padding: var(--space-4);
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

  .form-actions {
    display: flex;
    gap: var(--space-3);
  }

  .status-message {
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius-md);
    background: var(--color-accent-muted);
    color: var(--color-accent);
    font-size: var(--text-sm);
  }

  .status-message.error {
    background: oklch(65% 0.2 25 / 0.15);
    color: var(--color-danger);
  }
</style>
