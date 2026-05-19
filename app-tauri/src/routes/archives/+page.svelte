<script lang="ts">
  import { untrack } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { repoState, type RepoConfig } from '$lib/stores/repo.svelte';

  interface Archive {
    name: string;
    start: string;
    id: string;
  }

  let archives = $state<Archive[]>([]);
  let loading = $state(false);
  let error = $state('');
  let repoAvailable = $derived(repoState.hasRepo);

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
          <div class="archive-name">{archive.name}</div>
          <div class="archive-date">{archive.start}</div>
        </div>
      {/each}
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

  .archive-name {
    flex: 1;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    font-weight: 500;
  }

  .archive-date {
    color: var(--color-text-dim);
    font-size: var(--text-sm);
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

  .btn-secondary {
    background: var(--color-surface-hover);
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
  }

  .btn-secondary:hover:not(:disabled) {
    background: var(--color-surface-active);
    color: var(--color-text);
  }
</style>
