<script lang="ts">
  import { untrack } from 'svelte';
  import { invoke, Channel } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { confirm, open } from '@tauri-apps/plugin-dialog';
  import { repoState, isLocalRepo, type RepoConfig } from '$lib/stores/repo.svelte';
  import { notificationsState } from '$lib/stores/notifications.svelte';
  import { historyState } from '$lib/stores/history.svelte';
  import { profilesState } from '$lib/stores/profiles.svelte';
  import ArchiveBrowser from '$lib/components/ArchiveBrowser.svelte';
  import DiffViewer from '$lib/components/DiffViewer.svelte';
  import { formatBytes } from '$lib/format';
  import { explainConnectionError, repoHintContexts, type HintContext } from '$lib/connection-hints';

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
  // Plain-language suggestions under raw failure messages (see connection-hints).
  let errorHint = $state('');
  let restoreHint = $state('');
  let deleteHint = $state('');
  let compactHint = $state('');

  function hintFor(e: unknown, extra: HintContext[] = []): string {
    const ssh = repoState.config ? !isLocalRepo(repoState.config) : false;
    return explainConnectionError(String(e), repoHintContexts(ssh, extra)) ?? '';
  }
  let repoAvailable = $derived(repoState.hasRepo);

  let restoringArchive = $state('');
  let restoreStatus = $state('');
  let restoreFile = $state('');
  let restoreFileCount = $state(0);
  let restoreProgressMsg = $state('');
  let restoreCancelling = $state(false);
  let restoreWarnings = $state<string[]>([]);
  let appendOnly = $derived(profilesState.active?.hardening.append_only_declared ?? false);
  interface SearchMatch {
    archive_name: string;
    archive_start: string;
    entry: { path: string; size: number; entry_type: string };
  }
  let searchQuery = $state('');
  let searchMatches = $state<SearchMatch[]>([]);
  let searchStatus = $state('');
  let searching = $state(false);
  let selectedHistoryPath = $state<string | null>(null);
  let searchGeneration = 0;

  async function searchRestoreFiles() {
    const query = searchQuery.trim();
    if (!query || !repoState.config) return;
    const generation = ++searchGeneration;
    searching = true;
    searchMatches = [];
    selectedHistoryPath = null;
    searchStatus = 'Searching archives…';
    const channel = new Channel<{ matches: SearchMatch[]; archives_scanned: number }>();
    channel.onmessage = (batch) => {
      if (generation !== searchGeneration) return;
      searchMatches = [...searchMatches, ...batch.matches];
      searchStatus = `${searchMatches.length.toLocaleString()} matches · ${batch.archives_scanned} archives searched`;
    };
    try {
      const scanned = await invoke<number>('search_restore_files', {
        repo: repoState.config,
        query,
        requestId: `${Date.now()}-${generation}`,
        onBatch: channel,
      });
      if (generation === searchGeneration) {
        searchStatus = `${searchMatches.length.toLocaleString()} matches in ${scanned} archives`;
      }
    } catch (e) {
      if (generation === searchGeneration && !String(e).includes('cancelled')) {
        searchStatus = `Search failed: ${e}`;
      }
    } finally {
      if (generation === searchGeneration) searching = false;
    }
  }

  async function cancelRestoreSearch() {
    searchGeneration += 1;
    searching = false;
    searchStatus = 'Search cancelled.';
    await invoke<boolean>('cancel_restore_search');
  }

  function showVersionHistory(path: string) {
    selectedHistoryPath = path;
  }

  let visibleSearchMatches = $derived(
    selectedHistoryPath
      ? searchMatches.filter((match) => match.entry.path === selectedHistoryPath)
      : searchMatches,
  );

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

  // Archive comparison: pick a baseline archive, then a second to diff against.
  let compareFrom = $state<string | null>(null);
  let comparing = $state<{ a: string; b: string } | null>(null);

  // Repository compaction (reclaims space left by prune/delete).
  let compacting = $state(false);
  let compactStatus = $state('');

  // A borg-touching operation is in flight; gate the per-archive actions so two
  // operations can't fight over the repository lock.
  let busy = $derived(!!restoringArchive || !!deletingArchive || compacting);

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
    if (!compareFrom) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') compareFrom = null;
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  });

  function toggleCompare(name: string) {
    if (compareFrom === null) {
      compareFrom = name;
    } else if (compareFrom === name) {
      compareFrom = null;
    } else {
      comparing = { a: compareFrom, b: name };
      compareFrom = null;
    }
  }

  async function compactRepo() {
    if (!repoState.config || busy) return;
    compacting = true;
    compactStatus = '';
    compactHint = '';
    try {
      const summary = await invoke<string>('compact_repo', { repo: repoState.config });
      compactStatus = summary || 'Compaction complete.';
    } catch (e) {
      compactStatus = `Compaction failed: ${e}`;
      compactHint = hintFor(e);
    } finally {
      compacting = false;
    }
  }

  let loadedKey = $state<string | null>(null);
  let pendingRepo = $state<RepoConfig | null>(null);

  function repoKey(r: RepoConfig): string {
    return isLocalRepo(r)
      ? `local:${r.repo_path}`
      : `ssh:${r.ssh_user}@${r.ssh_host}:${r.ssh_port}/${r.repo_path}`;
  }

  $effect(() => {
    const r = repoState.config;
    const ready = r && (r.ssh_host || (isLocalRepo(r) && r.repo_path));
    // `untrack` so the load machinery's reads (loading/pendingRepo) don't make
    // this effect depend on them; it should re-run only when the repo changes.
    if (!ready) {
      untrack(() => {
        archives = [];
        error = '';
        errorHint = '';
        loadedKey = null;
      });
      return;
    }
    untrack(() => requestLoad(r));
  });

  function requestLoad(r: RepoConfig) {
    if (loading) {
      // A load is in flight; remember the latest repo so a profile switch
      // mid-load isn't dropped (it reloads when the current one finishes).
      pendingRepo = r;
      return;
    }
    void loadArchives(r);
  }

  async function loadArchives(r: RepoConfig) {
    const key = repoKey(r);
    // Switching to a different repo: drop the previous repo's list (and its
    // stale result banners) so the user never sees the wrong profile's archives.
    if (loadedKey && loadedKey !== key) {
      archives = [];
      deleteStatus = '';
      deleteHint = '';
      compactStatus = '';
      compactHint = '';
      if (!restoringArchive) {
        restoreStatus = '';
        restoreHint = '';
        restoreWarnings = [];
      }
    }
    loading = true;
    error = '';
    errorHint = '';
    try {
      archives = await invoke<Archive[]>('list_archives', { repo: r });
      loadedKey = key;
    } catch (e) {
      error = `Failed to load archives: ${e}`;
      errorHint = hintFor(e);
    } finally {
      loading = false;
      if (pendingRepo) {
        const next = pendingRepo;
        pendingRepo = null;
        if (repoKey(next) !== key) void loadArchives(next);
      }
    }
  }

  function refresh() {
    if (repoState.config) loadArchives(repoState.config);
  }

  async function restoreArchive(archiveName: string, paths?: string[], overwrite = false) {
    if (!repoState.config || restoringArchive) return;

    const dest = await open({ directory: true, multiple: false, title: 'Select restore destination' });
    if (!dest) return;
    if (overwrite) {
      const conflicts = paths?.length
        ? await invoke<Array<{ path: string; exists: boolean }>>('preview_restore_conflicts', {
            destination: dest as string,
            paths,
          })
        : [];
      const existing = conflicts.filter((conflict) => conflict.exists).length;
      const accepted = await confirm(
        `Overwrite mode can replace existing files${existing ? ` (${existing} conflicts detected)` : ''}. Continue?`,
        { title: 'Explicit overwrite confirmation', kind: 'warning' },
      );
      if (!accepted) return;
    }

    restoringArchive = archiveName;
    restoreCancelling = false;
    restoreWarnings = [];
    restoreHint = '';
    restoreStatus = paths && paths.length > 0
      ? `Restoring ${paths.length.toLocaleString()} selected items...`
      : 'Restoring...';
    restoreFile = '';
    restoreFileCount = 0;
    restoreProgressMsg = '';

    const startMs = Date.now();
    let unlisten: UnlistenFn | undefined;
    try {
      unlisten = await listen<RestoreProgress>('restore-progress', (event) => {
        const data = event.payload;
        if (data.type === 'archive_progress') {
          if (data.path) restoreFile = data.path;
          if (data.nfiles != null) restoreFileCount = data.nfiles;
        } else if (data.type === 'progress_percent') {
          // borg `extract` reports live progress here: `message` looks like
          // "20.0% Extracting: dir/file.txt". Surface it so the panel stays
          // informative (extract emits no `archive_progress` events).
          if (data.finished) {
            restoreStatus = 'Finalizing...';
            restoreProgressMsg = '';
          } else if (data.message) {
            restoreProgressMsg = data.message.trim();
          }
        }
      });

      const result = await invoke<{ warnings: string[]; destination: string }>('restore_archive', {
        repo: repoState.config,
        archiveName,
        destination: dest as string,
        paths: paths && paths.length > 0 ? paths : null,
        overwrite,
      });
      restoreWarnings = result.warnings;

      // borg `extract` reports progress ONLY via `progress_percent` events — it
      // never emits `archive_progress`/`nfiles` (unlike `create`). So
      // `restoreFileCount` stays 0 even on a perfectly good restore and is NOT a
      // reliable success signal. Trust the backend result instead: a resolved
      // promise means borg exited cleanly (rc 0, or rc 1 with warnings). A
      // selective restore whose paths matched nothing surfaces borg's
      // "include pattern never matched" text in `restoreWarnings`, so the user
      // is still told when nothing landed on disk.
      const fileCountLabel =
        restoreFileCount > 0 ? ` (${restoreFileCount.toLocaleString()} files)` : '';
      if (restoreWarnings.length > 0) {
        restoreStatus = `Restore finished with ${restoreWarnings.length} warning${restoreWarnings.length === 1 ? '' : 's'} — files written to ${result.destination}. See details below.`;
      } else {
        restoreStatus = `Restore complete — files written to ${result.destination}${fileCountLabel}.`;
      }
      notificationsState.notify(
        'Restore complete',
        `Archive "${archiveName}" restored to ${result.destination}.`,
      );
      historyState.record({
        id: `${Date.now()}`,
        timestamp: new Date().toISOString(),
        kind: 'restore',
        archive_name: archiveName,
        outcome: 'success',
        duration_seconds: Math.round((Date.now() - startMs) / 1000),
        ...(restoreFileCount > 0 ? { file_count: restoreFileCount } : {}),
      }).catch((err) => console.warn('Failed to record history:', err));
    } catch (e) {
      // Prefer the flag set when the user hit Cancel; fall back to matching the
      // backend's "operation cancelled" message.
      if (restoreCancelling || String(e).toLowerCase().includes('operation cancelled')) {
        restoreStatus =
          'Restore cancelled. Some files may already have been written to the destination folder.';
        // Cancelled restore is not a failure; skip history.
        return;
      }
      restoreStatus = `Restore failed: ${e}`;
      restoreHint = hintFor(e, ['restore']);
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
    deleteHint = '';

    try {
      await invoke('delete_archive', {
        repo: repoState.config,
        archiveName,
      });
      deleteStatus = `Deleted ${archiveName}`;
      archives = archives.filter((a) => a.name !== archiveName);
    } catch (e) {
      deleteStatus = `Delete failed: ${e}`;
      deleteHint = hintFor(e);
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
        <div class="header-actions">
          <button
            class="btn btn-secondary"
            onclick={compactRepo}
            disabled={busy || loading || appendOnly}
            title="Reclaim disk space left behind by deleted or pruned archives"
          >
            {appendOnly ? 'Compact requires server maintenance' : compacting ? 'Compacting...' : 'Compact'}
          </button>
          <button class="btn btn-secondary" onclick={refresh} disabled={busy || loading}>
            {loading ? 'Loading...' : 'Refresh'}
          </button>
        </div>
      {/if}
    </div>
  </header>

  {#if repoAvailable}
    <section class="restore-center">
      <div>
        <h2>Restore Confidence Center</h2>
        <p>Search filenames across every archive. Search results are streamed and are never saved to history.</p>
      </div>
      <form class="search-row" onsubmit={(event) => { event.preventDefault(); searchRestoreFiles(); }}>
        <input bind:value={searchQuery} placeholder="Search backed-up filenames" disabled={searching} />
        <button class="btn btn-primary" type="submit" disabled={searching || !searchQuery.trim()}>Search</button>
        {#if searching}
          <button class="btn btn-secondary" type="button" onclick={cancelRestoreSearch}>Cancel</button>
        {/if}
      </form>
      {#if searchStatus}<p class="search-status">{searchStatus}</p>{/if}
      {#if selectedHistoryPath}
        <div class="history-heading">
          <strong>Versions of <code>{selectedHistoryPath}</code></strong>
          <button class="btn btn-secondary" onclick={() => selectedHistoryPath = null}>All matches</button>
        </div>
      {/if}
      {#if visibleSearchMatches.length > 0}
        <div class="search-results">
          {#each visibleSearchMatches as match}
            <div class="search-result">
              <div>
                <code>{match.entry.path}</code>
                <small>{match.archive_start} · {formatBytes(match.entry.size)}</small>
              </div>
              <div class="search-actions">
                <button class="btn btn-secondary" onclick={() => showVersionHistory(match.entry.path)}>Versions</button>
                <button class="btn btn-restore" onclick={() => restoreArchive(match.archive_name, [match.entry.path])}>Restore safely</button>
                <button class="btn btn-secondary" onclick={() => restoreArchive(match.archive_name, [match.entry.path], true)}>Overwrite…</button>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </section>
  {/if}

  {#if compactStatus}
    <div class="restore-result" class:error={compactStatus.includes('failed')}>
      {compactStatus}
      {#if compactHint}<span class="result-hint">{compactHint}</span>{/if}
    </div>
  {/if}

  {#if !repoAvailable}
    <div class="empty-state">
      <p>No repository configured. <a href="/settings">Set up your connection</a> first.</p>
    </div>
  {:else if loading}
    <div class="loading-state">Loading archives...</div>
  {:else if error}
    <div class="error-banner">
      {error}
      {#if errorHint}<span class="result-hint">{errorHint}</span>{/if}
    </div>
  {:else if archives.length === 0}
    <div class="empty-state">
      <p>No archives found. <a href="/backup">Create your first backup</a> to get started.</p>
    </div>
  {:else}
    {#if compareFrom}
      <div class="compare-banner">
        Comparing from <code>{compareFrom}</code> — pick a second archive to see what
        changed, or press Esc to cancel.
      </div>
    {/if}
    <div class="archive-list">
      {#each archives as archive}
        <div class="archive-row" class:compare-base={compareFrom === archive.name}>
          <div class="archive-info">
            <div class="archive-name">{archive.name}</div>
            <div class="archive-date">{archive.start}</div>
          </div>
          <div class="archive-actions">
            <button
              class="btn btn-ghost"
              class:active={compareFrom === archive.name}
              onclick={() => toggleCompare(archive.name)}
              disabled={busy}
              title={compareFrom === null
                ? 'Pick this archive as the comparison baseline'
                : compareFrom === archive.name
                  ? 'Cancel comparison'
                  : `Compare ${compareFrom} → ${archive.name}`}
            >
              {compareFrom === archive.name
                ? '✕ Base'
                : compareFrom
                  ? 'Compare ▸'
                  : 'Compare'}
            </button>
            <button
              class="btn btn-secondary"
              onclick={() => browsingArchive = archive.name}
              disabled={busy}
              title="Browse archive contents"
            >
              Browse
            </button>
            <button
              class="btn btn-restore"
              onclick={() => restoreArchive(archive.name)}
              disabled={busy}
            >
              {restoringArchive === archive.name ? 'Restoring...' : 'Restore'}
            </button>
            <button
              class="btn btn-delete"
              onclick={() => confirmDeleteArchive = archive.name}
              disabled={busy}
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
        {#if deleteHint}<span class="result-hint">{deleteHint}</span>{/if}
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
        {#if restoreProgressMsg}
          <code class="restore-file">{restoreProgressMsg}</code>
        {:else if restoreFile}
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
        {#if restoreHint}<span class="result-hint">{restoreHint}</span>{/if}
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

  {#if comparing && repoState.config}
    <DiffViewer
      repo={repoState.config}
      archiveA={comparing.a}
      archiveB={comparing.b}
      onClose={() => comparing = null}
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
        <p>
          {appendOnly ? 'This logically deletes' : 'This will permanently delete'}
          <code>{confirmDeleteArchive}</code>.
          {appendOnly ? 'Physical data remains until trusted server-side maintenance compacts the repository.' : 'This cannot be undone.'}
        </p>
        <div class="modal-actions">
          <button bind:this={cancelBtn} class="btn btn-secondary" onclick={() => confirmDeleteArchive = null}>Cancel</button>
          <button class="btn btn-delete-confirm" onclick={confirmDelete}>Delete</button>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .restore-center {
    display: grid;
    gap: var(--space-3);
    margin-bottom: var(--space-6);
    padding: var(--space-5);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-surface-raised);
  }

  .restore-center h2,
  .restore-center p {
    margin: 0;
  }

  .restore-center p,
  .search-result small {
    color: var(--color-text-muted);
  }

  .search-row,
  .history-heading,
  .search-result,
  .search-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .search-row input {
    flex: 1;
    min-width: 0;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    color: var(--color-text);
  }

  .history-heading,
  .search-result {
    justify-content: space-between;
  }

  .search-results {
    display: grid;
    max-height: 360px;
    overflow: auto;
    border-top: 1px solid var(--color-border-subtle);
  }

  .search-result {
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--color-border-subtle);
  }

  .search-result > div:first-child {
    display: grid;
    min-width: 0;
  }

  .search-result code {
    overflow-wrap: anywhere;
  }

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

  .header-actions {
    display: flex;
    gap: var(--space-2);
    flex-shrink: 0;
  }

  .compare-banner {
    margin-bottom: var(--space-3);
    padding: var(--space-2) var(--space-4);
    border-radius: var(--radius-md);
    background: var(--color-accent-muted, var(--color-surface-hover));
    border: 1px solid var(--color-accent);
    color: var(--color-text);
    font-size: var(--text-sm);
  }

  .compare-banner code {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    background: var(--color-surface);
    padding: 1px 6px;
    border-radius: var(--radius-sm);
  }

  .archive-row.compare-base {
    border-color: var(--color-accent);
  }

  .btn-ghost {
    background: transparent;
    border: 1px solid var(--color-border);
    color: var(--color-text-muted);
  }

  .btn-ghost:hover:not(:disabled) {
    border-color: var(--color-accent);
    color: var(--color-text);
  }

  .btn-ghost.active {
    border-color: var(--color-accent);
    color: var(--color-accent);
    font-weight: 600;
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

  /* Plain-language suggestion shown under a raw error message. */
  .result-hint {
    display: block;
    margin-top: var(--space-2);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
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
