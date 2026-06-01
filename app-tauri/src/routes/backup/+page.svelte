<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { open } from '@tauri-apps/plugin-dialog';
  import { repoState, type RepoConfig } from '$lib/stores/repo.svelte';
  import { notificationsState } from '$lib/stores/notifications.svelte';
  import { historyState } from '$lib/stores/history.svelte';
  import { profilesState } from '$lib/stores/profiles.svelte';
  import { formatBytes } from '$lib/format';

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

  const EXCLUDE_PRESETS = ['*.tmp', '*.cache', 'node_modules', '.git', 'target', '__pycache__', '.venv', 'dist', 'build'];

  let sourcePaths = $state<string[]>([]);
  let excludes = $state<string[]>([]);
  let excludeInput = $state('');
  let isRunning = $state(false);
  let cancelling = $state(false);
  let status = $state('');
  let warnings = $state<string[]>([]);
  let cancelled = $state(false);
  let repo = $derived(repoState.config);
  let repoAvailable = $derived(repoState.hasRepo);

  async function cancelBackup() {
    if (!isRunning || cancelling) return;
    cancelling = true;
    status = 'Cancelling backup...';
    try {
      await invoke<boolean>('cancel_backup');
    } catch (e) {
      console.warn('Failed to request cancel:', e);
    }
  }

  function addExclude(pattern: string) {
    const trimmed = pattern.trim();
    if (trimmed && !excludes.includes(trimmed)) {
      excludes = [...excludes, trimmed];
    }
    excludeInput = '';
  }

  function removeExclude(index: number) {
    excludes = excludes.filter((_, i) => i !== index);
  }

  let currentFile = $state('');
  let fileCount = $state(0);
  let originalSize = $state(0);
  let compressedSize = $state(0);
  let deduplicatedSize = $state(0);

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
    cancelling = false;
    cancelled = false;
    warnings = [];
    status = 'Starting backup...';
    resetProgress();

    const startMs = Date.now();
    let archiveName = '';

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

      const template = profilesState.active?.archive_template ?? '';
      archiveName = await invoke<string>('preview_archive_name', { template });
      const result = await invoke<string[]>('create_backup', {
        repo,
        sourcePaths,
        archiveName,
        excludes,
        preBackup: profilesState.active?.pre_backup ?? null,
        postBackup: profilesState.active?.post_backup ?? null,
      });
      warnings = Array.isArray(result) ? result : [];
      // A resolved promise means the archive was created. A non-empty list
      // just means some files were skipped (locked/in-use) — still a success.
      status = warnings.length > 0
        ? `Backup completed with ${warnings.length} warning${warnings.length === 1 ? '' : 's'}.`
        : 'Backup completed successfully!';
      notificationsState.notify(
        'Backup complete',
        warnings.length > 0
          ? `${fileCount.toLocaleString()} files archived (${warnings.length} skipped).`
          : `${fileCount.toLocaleString()} files archived.`,
      );
      historyState.record({
        id: `${Date.now()}`,
        timestamp: new Date().toISOString(),
        kind: 'backup',
        archive_name: archiveName,
        outcome: 'success',
        duration_seconds: Math.round((Date.now() - startMs) / 1000),
        file_count: fileCount || undefined,
        original_size: originalSize || undefined,
      }).catch((err) => console.warn('Failed to record history:', err));
    } catch (e) {
      // Prefer the flag we set when the user hit Cancel; fall back to matching
      // the backend's "operation cancelled" message. Either way, a cancel is not
      // a failure.
      if (cancelling || String(e).toLowerCase().includes('operation cancelled')) {
        cancelled = true;
        status = 'Backup cancelled.';
        return;
      }
      status = `Backup failed: ${e}`;
      notificationsState.notify('Backup failed', 'See BorgUI for details.');
      historyState.record({
        id: `${Date.now()}`,
        timestamp: new Date().toISOString(),
        kind: 'backup',
        archive_name: archiveName || '(unnamed)',
        outcome: 'failure',
        duration_seconds: Math.round((Date.now() - startMs) / 1000),
        error_message: String(e),
      }).catch((err) => console.warn('Failed to record history:', err));
    } finally {
      unlisten?.();
      isRunning = false;
      cancelling = false;
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

    <div class="form-section">
      <span class="form-label">Exclude Patterns</span>
      {#if excludes.length > 0}
        <div class="chip-list">
          {#each excludes as pattern, i}
            <span class="chip">
              <code>{pattern}</code>
              <button type="button" class="chip-remove" onclick={() => removeExclude(i)} disabled={isRunning} aria-label="Remove pattern">✕</button>
            </span>
          {/each}
        </div>
      {/if}
      <div class="exclude-input-row">
        <input
          type="text"
          class="exclude-input"
          placeholder="e.g. *.log or node_modules"
          bind:value={excludeInput}
          disabled={isRunning}
          onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); addExclude(excludeInput); } }}
        />
        <button type="button" class="btn btn-secondary" onclick={() => addExclude(excludeInput)} disabled={isRunning || !excludeInput.trim()}>
          + Add
        </button>
      </div>
      <div class="preset-row">
        <span class="preset-label">Presets:</span>
        {#each EXCLUDE_PRESETS as preset}
          <button
            type="button"
            class="preset-chip"
            onclick={() => addExclude(preset)}
            disabled={isRunning || excludes.includes(preset)}
          >
            {preset}
          </button>
        {/each}
      </div>
    </div>

    <div class="form-actions">
      <button class="btn btn-primary" onclick={runBackup} disabled={isRunning || !repoAvailable}>
        {isRunning ? 'Backing up...' : 'Start Backup'}
      </button>
    </div>

    {#if isRunning}
      <div class="progress-panel">
        <div class="progress-top">
          <span class="progress-running">
            <span class="spinner" aria-hidden="true"></span>
            {cancelling ? 'Cancelling…' : 'Backing up…'}
          </span>
          <button
            type="button"
            class="btn btn-cancel"
            onclick={cancelBackup}
            disabled={cancelling}
          >
            {cancelling ? 'Cancelling…' : 'Cancel'}
          </button>
        </div>
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

    {#if warnings.length > 0 && !isRunning}
      <div class="warnings-panel">
        <div class="warnings-head">
          <span class="warnings-icon" aria-hidden="true">!</span>
          <div>
            <strong>Completed with {warnings.length} warning{warnings.length === 1 ? '' : 's'}</strong>
            <p>Your backup succeeded. These files were skipped — usually because they were open or in use (like email data or a document you had open). They'll be picked up next time.</p>
          </div>
        </div>
        <details class="warnings-details">
          <summary>Show skipped files</summary>
          <ul class="warnings-list">
            {#each warnings as w, i (i)}
              <li><code>{w}</code></li>
            {/each}
          </ul>
        </details>
      </div>
    {/if}

    {#if status}
      <div
        class="status-message"
        class:error={status.includes('failed') || status.includes('No repository')}
        class:success={status.includes('successfully') || status.includes('with warning')}
        class:cancelled={status.includes('cancelled')}
      >
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
    background: var(--color-warning-muted);
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
    background: var(--color-danger-muted);
    color: var(--color-danger);
  }

  .status-message.success {
    background: var(--color-success-muted);
    color: var(--color-success);
  }

  .status-message.cancelled {
    background: var(--color-surface-hover);
    color: var(--color-text-muted);
  }

  .btn-cancel {
    background: transparent;
    color: var(--color-danger);
    border: 1px solid var(--color-danger);
  }

  .btn-cancel:hover:not(:disabled) {
    background: var(--color-danger-muted);
  }

  .progress-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .progress-running {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-accent);
  }

  .spinner {
    width: 12px;
    height: 12px;
    border: 2px solid var(--color-accent-muted);
    border-top-color: var(--color-accent);
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .warnings-panel {
    background: var(--color-warning-muted);
    border: 1px solid var(--color-warning);
    border-radius: var(--radius-md);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .warnings-head {
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
  }

  .warnings-icon {
    flex-shrink: 0;
    width: 20px;
    height: 20px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--color-warning);
    color: var(--color-on-accent);
    font-weight: 700;
    font-size: var(--text-xs);
  }

  .warnings-head strong {
    display: block;
    color: var(--color-warning);
    font-size: var(--text-sm);
  }

  .warnings-head p {
    margin-top: var(--space-1);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    line-height: 1.5;
  }

  .warnings-details summary {
    cursor: pointer;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    list-style: none;
  }

  .warnings-details summary::-webkit-details-marker {
    display: none;
  }

  .warnings-details summary::before {
    content: '▸ ';
    color: var(--color-text-dim);
  }

  .warnings-details[open] summary::before {
    content: '▾ ';
  }

  .warnings-list {
    list-style: none;
    margin-top: var(--space-2);
    padding: var(--space-2);
    background: var(--color-bg);
    border-radius: var(--radius-sm);
    max-height: 160px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .warnings-list code {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    word-break: break-all;
  }

  .chip-list {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
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
  }

  .exclude-input {
    flex: 1;
    background: var(--color-surface);
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
</style>
