<script lang="ts">
  import { untrack } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { open } from '@tauri-apps/plugin-dialog';
  import { repoState, isLocalRepo, type RepoConfig } from '$lib/stores/repo.svelte';
  import { notificationsState } from '$lib/stores/notifications.svelte';
  import { historyState } from '$lib/stores/history.svelte';
  import ArchiveBrowser from '$lib/components/ArchiveBrowser.svelte';

  interface Archive {
    name: string;
    start: string;
    id: string;
  }

  interface RestoreProgress {
    type: string;
    nfiles?: number;
    path?: string;
    original_size?: number;
    finished?: boolean;
    message?: string;
  }

  let archives = $state<Archive[]>([]);
  let loading = $state(false);
  let error = $state('');
  let repoAvailable = $derived(repoState.hasRepo);

  let restoringArchive = $state('');
  let restoreStatus = $state('');
  let restoreFile = $state('');
  let restoreFileCount = $state(0);
  let restoreCancelling = $state(false);
  let restoreWarnings = $state<string[]>([]);

  async function cancelRestore() {
    if (!restoringArchive || restoreCancelling) return;
    restoreCancelling = true;
    restoreStatus = 'Cancelling restore...';
    try {
      await invoke<boolean>('cancel_restore');
    } catch (e) {
      console.warn('Failed to request cancel:', e);
    }
  }

  let deletingArchive = $state('');
  let confirmDeleteArchive = $state<string | null>(null);
  let deleteStatus = $state('');
  let cancelBtn = $state<HTMLButtonElement | null>(null);
  let browsingArchive = $state<string | null>(null);

  $effect(() => {
    if (!confirmDeleteArchive) return;

    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') confirmDeleteArchive = null;
    };
    window.addEventListener('keydown', handler);
    cancelBtn?.focus();

    return () => window.removeEventListener('keydown', handler);
  });

  $effect(() => {
    const r = repoState.config;
    const ready = r && (r.ssh_host || (isLocalRepo(r) && r.repo_path));
    if (ready && untrack(() => !loading)) {
      loadArchives(r);
    }
  });

  async function loadArchives(r: RepoConfig) {
    if (loading) return;
    loading = true;
    error = '';
    try {
      archives = await invoke<Archive[]>('list_archives', { repo: r });
    } catch (e) {
      error = `Failed to load archives: ${e}`;
    } finally {
      loading = false;
    }
  }

  function refresh() {
    if (repoState.config) loadArchives(repoState.config);
  }

  async function restoreArchive(archiveName: string, paths?: string[]) {
    if (!repoState.config || restoringArchive) return;

    const dest = await open({ directory: true, multiple: false, title: 'Select restore destination' });
    if (!dest) return;

    restoringArchive = archiveName;
    restoreCancelling = false;
    restoreWarnings = [];
    restoreStatus = paths && paths.length > 0
      ? `Restoring ${paths.length.toLocaleString()} selected items...`
      : 'Restoring...';
    restoreFile = '';
    restoreFileCount = 0;

    const startMs = Date.now();
    let unlisten: UnlistenFn | undefined;
    try {
      unlisten = await listen<RestoreProgress>('restore-progress', (event) => {
        const data = event.payload;
        if (data.type === 'archive_progress') {
          if (data.path) restoreFile = data.path;
          if (data.nfiles != null) restoreFileCount = data.nfiles;
        } else if (data.type === 'progress_percent' && data.finished) {
          restoreStatus = 'Finalizing...';
        }
      });

      const result = await invoke<string[]>('restore_archive', {
        repo: repoState.config,
        archiveName,
        destination: dest as string,
        paths: paths && paths.length > 0 ? paths : null,
      });
      restoreWarnings = Array.isArray(result) ? result : [];
      if (restoreFileCount === 0) {
        // borg exits 0 with no files extracted when no archive entries match
        // the supplied PATHs. Surface that explicitly so users aren't told
        // "restored" when nothing landed on disk.
        restoreStatus = `Restore exited cleanly but no files were extracted — check that your selection matches paths inside the archive.`;
        notificationsState.notify(
          'Restore extracted 0 files',
          `No archive entries matched the selection for "${archiveName}".`,
        );
        historyState.record({
          id: `${Date.now()}`,
          timestamp: new Date().toISOString(),
          kind: 'restore',
          archive_name: archiveName,
          outcome: 'failure',
          duration_seconds: Math.round((Date.now() - startMs) / 1000),
          error_message: 'borg extract matched 0 files',
        }).catch((err) => console.warn('Failed to record history:', err));
      } else {
        restoreStatus = restoreWarnings.length > 0
          ? `Restored ${restoreFileCount.toLocaleString()} files to ${dest} (${restoreWarnings.length} warning${restoreWarnings.length === 1 ? '' : 's'})`
          : `Restored ${restoreFileCount.toLocaleString()} files to ${dest}`;
        notificationsState.notify(
          'Restore complete',
          `Archive "${archiveName}" restored (${restoreFileCount.toLocaleString()} files).`,
        );
        historyState.record({
          id: `${Date.now()}`,
          timestamp: new Date().toISOString(),
          kind: 'restore',
          archive_name: archiveName,
          outcome: 'success',
          duration_seconds: Math.round((Date.now() - startMs) / 1000),
          file_count: restoreFileCount,
        }).catch((err) => console.warn('Failed to record history:', err));
      }
    } catch (e) {
      if (String(e).toLowerCase().includes('operation cancelled')) {
        restoreStatus =
          'Restore cancelled. Some files may already have been written to the destination folder.';
        // Cancelled restore is not a failure; skip history.
        return;
      }
      restoreStatus = `Restore failed: ${e}`;
      notificationsState.notify('Restore failed', 'See BorgUI for details.');
      historyState.record({
        id: `${Date.now()}`,
        timestamp: new Date().toISOString(),
        kind: 'restore',
        archive_name: archiveName,
        outcome: 'failure',
        duration_seconds: Math.round((Date.now() - startMs) / 1000),
        error_message: String(e),
      }).catch((err) => console.warn('Failed to record history:', err));
    } finally {
      unlisten?.();
      restoringArchive = '';
      restoreCancelling = false;
    }
  }

  async function confirmDelete() {
    const archiveName = confirmDeleteArchive;
    if (!archiveName || !repoState.config) return;

    confirmDeleteArchive = null;
    deletingArchive = archiveName;
    deleteStatus = '';

    try {
      await invoke('delete_archive', {
        repo: repoState.config,
        archiveName,
      });
      deleteStatus = `Deleted ${archiveName}`;
      archives = archives.filter((a) => a.name !== archiveName);
    } catch (e) {
      deleteStatus = `Delete failed: ${e}`;
    } finally {
      deletingArchive = '';
    }
  }
</script>

<div class="archives-page">
  <header class="page-header">
    <div class="header-row">
      <div>
        <h1>Archives</h1>
        <p class="subtitle">Browse and restore from backup archives</p>
      </div>
      {#if repoAvailable}
        <button class="btn btn-secondary" onclick={refresh} disabled={loading}>
          {loading ? 'Loading...' : 'Refresh'}
        </button>
      {/if}
    </div>
  </header>

  {#if !repoAvailable}
    <div class="empty-state">
      <p>No repository configured. <a href="/settings">Set up your connection</a> first.</p>
    </div>
  {:else if loading}
    <div class="loading-state">Loading archives...</div>
  {:else if error}
    <div class="error-banner">{error}</div>
  {:else if archives.length === 0}
    <div class="empty-state">
      <p>No archives found. <a href="/backup">Create your first backup</a> to get started.</p>
    </div>
  {:else}
    <div class="archive-list">
      {#each archives as archive}
        <div class="archive-row">
          <div class="archive-info">
            <div class="archive-name">{archive.name}</div>
            <div class="archive-date">{archive.start}</div>
          </div>
          <div class="archive-actions">
            <button
              class="btn btn-secondary"
              onclick={() => browsingArchive = archive.name}
              disabled={!!restoringArchive || !!deletingArchive}
              title="Browse archive contents"
            >
              Browse
            </button>
            <button
              class="btn btn-restore"
              onclick={() => restoreArchive(archive.name)}
              disabled={!!restoringArchive || !!deletingArchive}
            >
              {restoringArchive === archive.name ? 'Restoring...' : 'Restore'}
            </button>
            <button
              class="btn btn-delete"
              onclick={() => confirmDeleteArchive = archive.name}
              disabled={!!restoringArchive || !!deletingArchive}
              title="Delete archive"
            >
              {deletingArchive === archive.name ? 'Deleting...' : 'Delete'}
            </button>
          </div>
        </div>
      {/each}
    </div>

    {#if deleteStatus}
      <div class="restore-result" class:error={deleteStatus.includes('failed')}>
        {deleteStatus}
      </div>
    {/if}

    {#if restoringArchive}
      <div class="restore-progress">
        <div class="restore-progress-top">
          <div class="restore-progress-header">
            {restoreCancelling ? 'Cancelling restore of' : 'Restoring'}: <code>{restoringArchive}</code>
          </div>
          <button
            type="button"
            class="btn btn-cancel"
            onclick={cancelRestore}
            disabled={restoreCancelling}
          >
            {restoreCancelling ? 'Cancelling…' : 'Cancel'}
          </button>
        </div>
        {#if restoreFile}
          <code class="restore-file">{restoreFile}</code>
        {/if}
        {#if restoreFileCount > 0}
          <span class="restore-count">{restoreFileCount.toLocaleString()} files extracted</span>
        {/if}
      </div>
    {/if}

    {#if restoreWarnings.length > 0 && !restoringArchive}
      <div class="warnings-panel">
        <div class="warnings-head">
          <span class="warnings-icon" aria-hidden="true">!</span>
          <div>
            <strong>Completed with {restoreWarnings.length} warning{restoreWarnings.length === 1 ? '' : 's'}</strong>
            <p>Your files were restored. These notes are usually harmless — for example a pattern that matched nothing.</p>
          </div>
        </div>
        <details class="warnings-details">
          <summary>Show details</summary>
          <ul class="warnings-list">
            {#each restoreWarnings as w, i (i)}
              <li><code>{w}</code></li>
            {/each}
          </ul>
        </details>
      </div>
    {/if}

    {#if restoreStatus && !restoringArchive}
      <div
        class="restore-result"
        class:error={restoreStatus.includes('failed')}
        class:warning={restoreStatus.includes('no files were extracted') || restoreStatus.includes('warning')}
        class:cancelled={restoreStatus.includes('cancelled')}
      >
        {restoreStatus}
      </div>
    {/if}
  {/if}

  {#if browsingArchive && repoState.config}
    <ArchiveBrowser
      repo={repoState.config}
      archiveName={browsingArchive}
      onClose={() => browsingArchive = null}
      onRestore={(paths) => {
        const name = browsingArchive!;
        browsingArchive = null;
        restoreArchive(name, paths);
      }}
    />
  {/if}

  {#if confirmDeleteArchive}
    <div
      class="modal-backdrop"
      onclick={() => confirmDeleteArchive = null}
      role="presentation"
    >
      <div
        class="modal"
        onclick={(e) => e.stopPropagation()}
        onkeydown={() => {}}
        role="dialog"
        tabindex="-1"
        aria-modal="true"
        aria-labelledby="delete-title"
      >
        <h2 id="delete-title">Delete archive?</h2>
        <p>This will permanently delete <code>{confirmDeleteArchive}</code>. This cannot be undone.</p>
        <div class="modal-actions">
          <button bind:this={cancelBtn} class="btn btn-secondary" onclick={() => confirmDeleteArchive = null}>Cancel</button>
          <button class="btn btn-delete-confirm" onclick={confirmDelete}>Delete</button>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .archives-page {
    max-width: 800px;
  }

  .page-header {
    margin-bottom: var(--space-8);
  }

  .header-row {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
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

  .empty-state {
    background: var(--color-surface);
    border: 1px dashed var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-8);
    text-align: center;
    color: var(--color-text-muted);
  }

  .loading-state {
    padding: var(--space-8);
    text-align: center;
    color: var(--color-text-dim);
  }

  .error-banner {
    background: var(--color-danger-muted);
    color: var(--color-danger);
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius-md);
    font-size: var(--text-sm);
  }

  .archive-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .archive-row {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
  }

  .archive-info {
    flex: 1;
    display: flex;
    align-items: center;
    gap: var(--space-4);
    min-width: 0;
  }

  .archive-name {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .archive-date {
    color: var(--color-text-dim);
    font-size: var(--text-sm);
    flex-shrink: 0;
  }

  .archive-actions {
    display: flex;
    gap: var(--space-2);
    flex-shrink: 0;
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
    max-width: 420px;
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
  }

  .restore-progress {
    margin-top: var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .restore-progress-header {
    font-size: var(--text-sm);
    font-weight: 500;
  }

  .restore-file {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .restore-count {
    font-size: var(--text-sm);
    color: var(--color-accent);
    font-weight: 600;
    font-family: var(--font-mono);
  }

  .restore-result {
    margin-top: var(--space-4);
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius-md);
    background: var(--color-success-muted);
    color: var(--color-success);
    font-size: var(--text-sm);
  }

  .restore-result.error {
    background: var(--color-danger-muted);
    color: var(--color-danger);
  }

  .restore-result.warning {
    background: var(--color-warning-muted);
    color: var(--color-warning);
  }

  .restore-result.cancelled {
    background: var(--color-surface-hover);
    color: var(--color-text-muted);
  }

  .restore-progress-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .btn-cancel {
    background: transparent;
    color: var(--color-danger);
    border: 1px solid var(--color-danger);
  }

  .btn-cancel:hover:not(:disabled) {
    background: var(--color-danger-muted);
  }

  .warnings-panel {
    margin-top: var(--space-4);
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
</style>
