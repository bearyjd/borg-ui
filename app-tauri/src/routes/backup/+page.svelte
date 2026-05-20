<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { open } from '@tauri-apps/plugin-dialog';
  import { repoState, type RepoConfig } from '$lib/stores/repo.svelte';

  interface ArchiveProgress {
    type: 'archive_progress';
    original_size?: number;
    compressed_size?: number;
    deduplicated_size?: number;
    nfiles?: number;
    path?: string;
  }

  interface PercentProgress {
    type: 'progress_percent';
    finished: boolean;
    message?: string;
    current?: number;
    total?: number;
  }

  interface LogMessage {
    type: 'log_message';
    levelname: string;
    message: string;
  }

  type ProgressEvent = ArchiveProgress | PercentProgress | LogMessage;

  let sourcePaths = $state<string[]>([]);
  let isRunning = $state(false);
  let status = $state('');
  let repo = $derived(repoState.config);
  let repoAvailable = $derived(repoState.hasRepo);

  let currentFile = $state('');
  let fileCount = $state(0);
  let originalSize = $state(0);
  let compressedSize = $state(0);
  let deduplicatedSize = $state(0);

  function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(1024));
    return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
  }

  function resetProgress() {
    currentFile = '';
    fileCount = 0;
    originalSize = 0;
    compressedSize = 0;
    deduplicatedSize = 0;
  }

  async function addFolder() {
    const selected = await open({ directory: true, multiple: false, title: 'Select folder to back up' });
    if (selected && !sourcePaths.includes(selected as string)) {
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
    resetProgress();

    let unlisten: UnlistenFn | undefined;
    try {
      unlisten = await listen<ProgressEvent>('backup-progress', (event) => {
        const data = event.payload;
        if (data.type === 'archive_progress') {
          if (data.path) currentFile = data.path;
          if (data.nfiles != null) fileCount = data.nfiles;
          if (data.original_size != null) originalSize = data.original_size;
          if (data.compressed_size != null) compressedSize = data.compressed_size;
          if (data.deduplicated_size != null) deduplicatedSize = data.deduplicated_size;
          status = 'Backing up...';
        } else if (data.type === 'progress_percent') {
          if (data.finished) {
            status = 'Finalizing...';
          } else if (data.message) {
            status = data.message;
          }
        } else if (data.type === 'log_message') {
          if (data.levelname === 'WARNING' || data.levelname === 'ERROR') {
            status = data.message;
          }
        }
      });

      const ts = new Date().toISOString().replace(/[:.]/g, '-');
      const suffix = Math.random().toString(36).slice(2, 6);
      const archiveName = `backup-${ts}-${suffix}`;
      await invoke('create_backup', {
        repo,
        sourcePaths,
        archiveName,
      });
      status = 'Backup completed successfully!';
    } catch (e) {
      status = `Backup failed: ${e}`;
    } finally {
      unlisten?.();
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

    {#if isRunning && (fileCount > 0 || currentFile)}
      <div class="progress-panel">
        {#if currentFile}
          <div class="progress-file">
            <span class="progress-label">Current file</span>
            <code class="progress-path">{currentFile}</code>
          </div>
        {/if}
        <div class="progress-stats">
          {#if fileCount > 0}
            <div class="stat">
              <span class="stat-value">{fileCount.toLocaleString()}</span>
              <span class="stat-label">files</span>
            </div>
          {/if}
          {#if originalSize > 0}
            <div class="stat">
              <span class="stat-value">{formatBytes(originalSize)}</span>
              <span class="stat-label">original</span>
            </div>
          {/if}
          {#if compressedSize > 0}
            <div class="stat">
              <span class="stat-value">{formatBytes(compressedSize)}</span>
              <span class="stat-label">compressed</span>
            </div>
          {/if}
          {#if deduplicatedSize > 0}
            <div class="stat">
              <span class="stat-value">{formatBytes(deduplicatedSize)}</span>
              <span class="stat-label">deduplicated</span>
            </div>
          {/if}
        </div>
      </div>
    {/if}

    {#if status}
      <div class="status-message" class:error={status.includes('failed') || status.includes('No repository')} class:success={status.includes('successfully')}>
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

  .progress-panel {
    background: var(--color-surface);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .progress-file {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }

  .progress-label {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--color-text-dim);
    font-weight: 600;
  }

  .progress-path {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .progress-stats {
    display: flex;
    gap: var(--space-6);
    flex-wrap: wrap;
  }

  .stat {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .stat-value {
    font-size: var(--text-base);
    font-weight: 600;
    font-family: var(--font-mono);
    color: var(--color-accent);
  }

  .stat-label {
    font-size: var(--text-xs);
    color: var(--color-text-dim);
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

  .status-message.success {
    background: oklch(75% 0.15 145 / 0.15);
    color: var(--color-success);
  }
</style>
