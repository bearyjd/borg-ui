<script lang="ts">
  import { untrack } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { open } from '@tauri-apps/plugin-dialog';
  import { repoState, type RepoConfig } from '$lib/stores/repo.svelte';
  import { notificationsState } from '$lib/stores/notifications.svelte';

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

  let deletingArchive = $state('');
  let confirmDeleteArchive = $state<string | null>(null);
  let deleteStatus = $state('');
  let cancelBtn = $state<HTMLButtonElement | null>(null);

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
    if (r && r.ssh_host && untrack(() => !loading)) {
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

  async function restoreArchive(archiveName: string) {
    if (!repoState.config || restoringArchive) return;

    const dest = await open({ directory: true, multiple: false, title: 'Select restore destination' });
    if (!dest) return;

    restoringArchive = archiveName;
    restoreStatus = 'Restoring...';
    restoreFile = '';
    restoreFileCount = 0;

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

      await invoke('restore_archive', {
        repo: repoState.config,
        archiveName,
        destination: dest as string,
      });
      restoreStatus = `Restored to ${dest}`;
      notificationsState.notify(
        'Restore complete',
        `Archive "${archiveName}" restored.`,
      );
    } catch (e) {
      restoreStatus = `Restore failed: ${e}`;
      notificationsState.notify('Restore failed', 'See BorgUI for details.');
    } finally {
      unlisten?.();
      restoringArchive = '';
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
        <div class="restore-progress-header">Restoring: <code>{restoringArchive}</code></div>
        {#if restoreFile}
          <code class="restore-file">{restoreFile}</code>
        {/if}
        {#if restoreFileCount > 0}
          <span class="restore-count">{restoreFileCount.toLocaleString()} files extracted</span>
        {/if}
      </div>
    {/if}

    {#if restoreStatus && !restoringArchive}
      <div class="restore-result" class:error={restoreStatus.includes('failed')}>
        {restoreStatus}
      </div>
    {/if}
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
    background: oklch(65% 0.2 25 / 0.15);
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

  .btn {
    padding: var(--space-2) var(--space-4);
    border-radius: var(--radius-md);
    font-weight: 500;
    font-size: var(--text-sm);
    transition: all var(--duration-fast) var(--ease-out);
    flex-shrink: 0;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
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

  .btn-restore {
    background: var(--color-accent-muted);
    color: var(--color-accent);
    border: 1px solid transparent;
  }

  .btn-restore:hover:not(:disabled) {
    background: var(--color-accent);
    color: oklch(14% 0 0);
  }

  .archive-actions {
    display: flex;
    gap: var(--space-2);
    flex-shrink: 0;
  }

  .btn-delete {
    background: transparent;
    color: var(--color-text-dim);
    border: 1px solid var(--color-border);
  }

  .btn-delete:hover:not(:disabled) {
    background: oklch(65% 0.2 25 / 0.12);
    color: var(--color-danger);
    border-color: var(--color-danger);
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

  .btn-delete-confirm {
    background: var(--color-danger);
    color: oklch(14% 0 0);
    border: 1px solid var(--color-danger);
  }

  .btn-delete-confirm:hover:not(:disabled) {
    background: oklch(60% 0.22 25);
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
    background: oklch(75% 0.15 145 / 0.15);
    color: var(--color-success);
    font-size: var(--text-sm);
  }

  .restore-result.error {
    background: oklch(65% 0.2 25 / 0.15);
    color: var(--color-danger);
  }
</style>
