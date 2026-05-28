<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import type { RepoConfig } from '$lib/stores/repo.svelte';
  import {
    buildTree,
    collectFilePaths,
    folderState,
    type ArchiveEntry,
    type TreeNode,
  } from '$lib/archive-tree';

  interface Props {
    repo: RepoConfig;
    archiveName: string;
    onClose: () => void;
    onRestore: (paths: string[]) => void;
  }

  let { repo, archiveName, onClose, onRestore }: Props = $props();

  let entries = $state<ArchiveEntry[]>([]);
  let loading = $state(true);
  let error = $state('');
  let selected = $state(new Set<string>());
  let expanded = $state(new Set<string>(['']));
  let cancelBtn = $state<HTMLButtonElement | null>(null);

  let tree = $derived(buildTree(entries));
  let totalFiles = $derived(collectFilePaths(tree).length);

  $effect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', handler);
    cancelBtn?.focus();
    return () => window.removeEventListener('keydown', handler);
  });

  $effect(() => {
    void load();
  });

  async function load() {
    loading = true;
    error = '';
    try {
      entries = await invoke<ArchiveEntry[]>('list_archive_contents', {
        repo,
        archiveName,
      });
    } catch (e) {
      error = `Failed to load archive contents: ${e}`;
    } finally {
      loading = false;
    }
  }

  function toggleExpanded(path: string) {
    const next = new Set(expanded);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    expanded = next;
  }

  function toggleNode(node: TreeNode) {
    const next = new Set(selected);
    if (node.isDir) {
      const leaves = collectFilePaths(node);
      const allSelected = leaves.length > 0 && leaves.every((p) => next.has(p));
      if (allSelected) {
        for (const p of leaves) next.delete(p);
      } else {
        for (const p of leaves) next.add(p);
      }
    } else if (next.has(node.path)) {
      next.delete(node.path);
    } else {
      next.add(node.path);
    }
    selected = next;
  }

  function selectAll() {
    selected = new Set(collectFilePaths(tree));
  }

  function clearAll() {
    selected = new Set();
  }

  function handleRestore() {
    if (selected.size === 0) return;
    onRestore([...selected]);
  }

  function formatSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
  }
</script>

<div class="modal-backdrop" onclick={onClose} role="presentation">
  <div
    class="modal browser"
    onclick={(e) => e.stopPropagation()}
    onkeydown={() => {}}
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-labelledby="browser-title"
  >
    <header class="browser-header">
      <div>
        <h2 id="browser-title">Browse archive</h2>
        <code class="archive-name">{archiveName}</code>
      </div>
      <div class="browser-stats">
        {#if !loading && !error}
          <span>{selected.size.toLocaleString()} / {totalFiles.toLocaleString()} files</span>
        {/if}
      </div>
    </header>

    {#if loading}
      <div class="browser-state">Loading archive contents…</div>
    {:else if error}
      <div class="error-banner">{error}</div>
    {:else if entries.length === 0}
      <div class="browser-state">Archive is empty.</div>
    {:else}
      <div class="browser-actions">
        <button class="link-btn" onclick={selectAll}>Select all</button>
        <button class="link-btn" onclick={clearAll} disabled={selected.size === 0}>Clear</button>
      </div>
      <div class="tree-scroll">
        <ul class="tree-root">
          {#each tree.children as node (node.path)}
            {@render renderNode(node, 0)}
          {/each}
        </ul>
      </div>
    {/if}

    <footer class="browser-footer">
      <button bind:this={cancelBtn} class="btn btn-secondary" onclick={onClose}>Cancel</button>
      <button
        class="btn btn-restore"
        onclick={handleRestore}
        disabled={loading || selected.size === 0}
      >
        Restore selected ({selected.size.toLocaleString()})
      </button>
    </footer>
  </div>
</div>

{#snippet renderNode(node: TreeNode, depth: number)}
  {@const state = node.isDir ? folderState(node, selected) : null}
  {@const checked = node.isDir
    ? state!.total > 0 && state!.selected === state!.total
    : selected.has(node.path)}
  {@const indeterminate = node.isDir && state!.selected > 0 && state!.selected < state!.total}
  {@const isOpen = expanded.has(node.path)}
  <li>
    <div class="row" style="padding-left: {depth * 1.25}rem">
      {#if node.isDir}
        <button
          class="disclosure"
          onclick={() => toggleExpanded(node.path)}
          aria-label={isOpen ? 'Collapse' : 'Expand'}
          aria-expanded={isOpen}
        >
          {isOpen ? '▾' : '▸'}
        </button>
      {:else}
        <span class="disclosure spacer"></span>
      {/if}
      <input
        type="checkbox"
        {checked}
        indeterminate={indeterminate || undefined}
        onchange={() => toggleNode(node)}
        aria-label={node.name}
      />
      <span class="icon">{node.isDir ? '📁' : '📄'}</span>
      <span class="name" title={node.path}>{node.name}</span>
      {#if !node.isDir}
        <span class="size">{formatSize(node.size)}</span>
      {/if}
    </div>
    {#if node.isDir && isOpen}
      <ul>
        {#each node.children as child (child.path)}
          {@render renderNode(child, depth + 1)}
        {/each}
      </ul>
    {/if}
  </li>
{/snippet}

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: var(--color-backdrop);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .modal.browser {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-5);
    width: min(720px, 92vw);
    max-height: 85vh;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .browser-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: var(--space-4);
  }

  .browser-header h2 {
    font-size: var(--text-lg);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .archive-name {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    background: var(--color-surface-hover);
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    display: inline-block;
    margin-top: var(--space-1);
  }

  .browser-stats {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  .browser-state {
    padding: var(--space-6);
    text-align: center;
    color: var(--color-text-dim);
    font-size: var(--text-sm);
  }

  .browser-actions {
    display: flex;
    gap: var(--space-3);
  }

  .link-btn {
    background: transparent;
    border: none;
    color: var(--color-accent);
    font-size: var(--text-sm);
    cursor: pointer;
    padding: 0;
  }

  .link-btn:disabled {
    color: var(--color-text-dim);
    cursor: not-allowed;
  }

  .link-btn:hover:not(:disabled) {
    text-decoration: underline;
  }

  .tree-scroll {
    flex: 1;
    overflow-y: auto;
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-1);
    background: var(--color-surface-hover);
    min-height: 200px;
  }

  ul {
    list-style: none;
    padding: 0;
    margin: 0;
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 2px var(--space-2);
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
  }

  .row:hover {
    background: var(--color-surface-active);
  }

  .disclosure {
    width: 1rem;
    background: transparent;
    border: none;
    color: var(--color-text-dim);
    cursor: pointer;
    padding: 0;
    font-size: var(--text-xs);
    flex-shrink: 0;
  }

  .disclosure.spacer {
    display: inline-block;
    cursor: default;
  }

  .icon {
    flex-shrink: 0;
  }

  .name {
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .size {
    color: var(--color-text-dim);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    flex-shrink: 0;
  }

  .browser-footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }

  .error-banner {
    background: var(--color-danger-muted);
    color: var(--color-danger);
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius-md);
    font-size: var(--text-sm);
  }

</style>
