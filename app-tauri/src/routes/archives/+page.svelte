<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';

  interface Archive {
    name: string;
    start: string;
    id: string;
  }

  let archives = $state<Archive[]>([]);
  let loading = $state(false);
  let error = $state('');
</script>

<div class="archives-page">
  <header class="page-header">
    <h1>Archives</h1>
    <p class="subtitle">Browse and restore from backup archives</p>
  </header>

  {#if error}
    <div class="error-banner">{error}</div>
  {:else if archives.length === 0}
    <div class="empty-state">
      <p>No archives found. Connect a repository in <a href="/settings">Settings</a> first.</p>
    </div>
  {:else}
    <div class="archive-list">
      {#each archives as archive}
        <div class="archive-row">
          <div class="archive-name">{archive.name}</div>
          <div class="archive-date">{archive.start}</div>
          <button class="btn btn-secondary">Browse</button>
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
    padding: var(--space-1) var(--space-3);
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    font-weight: 500;
    transition: all var(--duration-fast) var(--ease-out);
  }

  .btn-secondary {
    background: var(--color-surface-hover);
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
  }

  .btn-secondary:hover {
    background: var(--color-surface-active);
    color: var(--color-text);
  }
</style>
