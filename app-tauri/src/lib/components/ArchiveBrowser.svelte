<script lang="ts">
  import { invoke, Channel } from '@tauri-apps/api/core';
  import type { RepoConfig } from '$lib/stores/repo.svelte';
  import { formatBytes } from '$lib/format';
  import {
    buildTree,
    flattenVisible,
    Selection,
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

  // Fixed-height virtual scrolling: only the rows in (or near) the viewport are
  // ever in the DOM, so an archive with 100k+ entries renders a constant ~40
  // nodes regardless of how much is expanded.
  const ROW_H = 26;
  const OVERSCAN = 8;

  let tree = $state.raw<TreeNode | null>(null);
  let selection = $state.raw<Selection | null>(null);
  let loading = $state(true);
  let error = $state('');
  let loadedCount = $state(0);
  let isEmpty = $state(false);
  let expanded = $state(new Set<string>());
  // Selection mutates `selection` in place; bump this to re-derive views.
  let selVersion = $state(0);
  let cancelBtn = $state<HTMLButtonElement | null>(null);

  let scrollTop = $state(0);
  let viewportH = $state(0);

  // Monotonic token: a newer load() invalidates any still-in-flight earlier one
  // so a slow stream for a previous archive can't clobber the current tree.
  let loadGen = 0;

  let totalFiles = $derived(tree ? tree.leafCount : 0);
  let selectedCount = $derived.by(() => {
    void selVersion;
    return selection ? selection.size : 0;
  });

  // Flattened list of currently-visible (expanded) rows; the scroller windows
  // over this. Cheap to rebuild — it's references, not DOM.
  let flat = $derived(tree ? flattenVisible(tree, expanded) : []);
  // Clamp the scroll offset used for the window math: collapsing a deep folder
  // shrinks `flat`, which can leave `scrollTop` past the new end (the native
  // clamp doesn't always re-fire onscroll). Without this the window slice would
  // go empty and the list would render blank until the next scroll.
  let maxScrollTop = $derived(Math.max(0, flat.length * ROW_H - viewportH));
  let clampedTop = $derived(Math.min(scrollTop, maxScrollTop));
  let startIndex = $derived(Math.max(0, Math.floor(clampedTop / ROW_H) - OVERSCAN));
  let endIndex = $derived(Math.min(flat.length, Math.ceil((clampedTop + viewportH) / ROW_H) + OVERSCAN));

  // Per-row display state for just the windowed slice. Re-runs on selection
  // change (selVersion) but only over the ~40 visible rows.
  let rows = $derived.by(() => {
    void selVersion;
    const sel = selection;
    return flat.slice(startIndex, endIndex).map(({ node, depth }) => {
      if (node.isDir) {
        const under = sel ? sel.selectedUnder(node) : 0;
        const total = node.leafCount;
        return {
          node,
          depth,
          checked: total > 0 && under === total,
          indeterminate: under > 0 && under < total,
        };
      }
      return {
        node,
        depth,
        checked: sel ? sel.isSelected(node.path) : false,
        indeterminate: false,
      };
    });
  });

  $effect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', handler);
    cancelBtn?.focus();
    return () => window.removeEventListener('keydown', handler);
  });

  $effect(() => {
    // Re-load when the target archive changes. Keyed on the archive name only —
    // the repo can't change while the browser is open, and reading the `repo`
    // object here would re-fire on any identity churn from the parent.
    archiveName;
    void load();
  });

  async function load() {
    const gen = ++loadGen;
    loading = true;
    error = '';
    loadedCount = 0;
    isEmpty = false;
    tree = null;
    selection = null;
    expanded = new Set();
    selVersion = 0;
    scrollTop = 0;

    // Accumulate batches into a plain array (not reactive state) so streaming a
    // huge listing doesn't churn the UI; only `loadedCount` drives the progress
    // text. The tree is built once, when the stream completes.
    const incoming: ArchiveEntry[] = [];
    const channel = new Channel<ArchiveEntry[]>();
    channel.onmessage = (batch) => {
      if (gen !== loadGen) return; // superseded by a newer load
      for (const entry of batch) incoming.push(entry);
      loadedCount = incoming.length;
    };

    try {
      await invoke<number>('stream_archive_contents', {
        repo,
        archiveName,
        onBatch: channel,
      });
      if (gen !== loadGen) return;
      isEmpty = incoming.length === 0;
      const built = buildTree(incoming);
      tree = built;
      selection = new Selection(built);
    } catch (e) {
      if (gen !== loadGen) return;
      error = `Failed to load archive contents: ${e}`;
    } finally {
      if (gen === loadGen) loading = false;
    }
  }

  function toggleExpanded(path: string) {
    const next = new Set(expanded);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    expanded = next;
  }

  function toggleSelect(node: TreeNode) {
    if (!selection) return;
    selection.toggle(node);
    selVersion++;
  }

  function selectAll() {
    if (!selection) return;
    selection.selectAll();
    selVersion++;
  }

  function clearAll() {
    if (!selection) return;
    selection.clear();
    selVersion++;
  }

  function handleRestore() {
    if (!selection || selection.size === 0) return;
    onRestore(selection.selectedPaths());
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
          <span>{selectedCount.toLocaleString()} / {totalFiles.toLocaleString()} files</span>
        {/if}
      </div>
    </header>

    {#if loading}
      <div class="browser-state">
        Loading archive contents…
        {#if loadedCount > 0}
          <span class="loaded-count">{loadedCount.toLocaleString()} files</span>
        {/if}
      </div>
    {:else if error}
      <div class="error-banner">{error}</div>
    {:else if isEmpty}
      <div class="browser-state">Archive is empty.</div>
    {:else}
      <p class="browser-help">
        Tick the files and folders you want back. "Restore selected" then asks
        where to put them and rebuilds their original folder structure inside
        the folder you choose.
      </p>
      <div class="browser-actions">
        <button class="link-btn" onclick={selectAll}>Select all</button>
        <button class="link-btn" onclick={clearAll} disabled={selectedCount === 0}>Clear</button>
      </div>
      <div
        class="tree-scroll"
        role="tree"
        aria-label="Archive contents"
        tabindex="0"
        bind:clientHeight={viewportH}
        onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
      >
        <div class="tree-spacer" style="height: {flat.length * ROW_H}px;">
          <div class="tree-window" style="transform: translateY({startIndex * ROW_H}px);">
            {#each rows as row (row.node.path)}
              <div
                class="row"
                role="treeitem"
                aria-level={row.depth + 1}
                aria-selected={row.checked}
                aria-expanded={row.node.isDir ? expanded.has(row.node.path) : undefined}
                style="padding-left: {row.depth * 1.25}rem; height: {ROW_H}px;"
              >
                {#if row.node.isDir}
                  <button
                    class="disclosure"
                    onclick={() => toggleExpanded(row.node.path)}
                    aria-label={expanded.has(row.node.path) ? 'Collapse' : 'Expand'}
                  >
                    {expanded.has(row.node.path) ? '▾' : '▸'}
                  </button>
                {:else}
                  <span class="disclosure spacer"></span>
                {/if}
                <input
                  type="checkbox"
                  checked={row.checked}
                  indeterminate={row.indeterminate || undefined}
                  onchange={() => toggleSelect(row.node)}
                  aria-label={row.node.name}
                />
                <span class="icon" class:dir={row.node.isDir} aria-hidden="true">
                  {#if row.node.isDir}
                    <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round">
                      <path d="M1.5 4.5a1 1 0 0 1 1-1h3l1.5 1.5h6a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" />
                    </svg>
                  {:else}
                    <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round">
                      <path d="M3.5 1.5h6L13 5v9.5a.5.5 0 0 1-.5.5h-9a.5.5 0 0 1-.5-.5V2a.5.5 0 0 1 .5-.5z" />
                      <path d="M9.5 1.5V5H13" />
                    </svg>
                  {/if}
                </span>
                <span class="name" title={row.node.path}>{row.node.name}</span>
                {#if !row.node.isDir}
                  <span class="size">{formatBytes(row.node.size)}</span>
                {/if}
              </div>
            {/each}
          </div>
        </div>
      </div>
    {/if}

    <footer class="browser-footer">
      <button bind:this={cancelBtn} class="btn btn-secondary" onclick={onClose}>Cancel</button>
      <button
        class="btn btn-restore"
        onclick={handleRestore}
        disabled={loading || selectedCount === 0}
      >
        Restore selected ({selectedCount.toLocaleString()})
      </button>
    </footer>
  </div>
</div>

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

  .loaded-count {
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    margin-left: var(--space-2);
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

  /* Full-height spacer reserves scroll range for all rows; the window is
     absolutely positioned and translated to the first visible row. */
  .tree-spacer {
    position: relative;
    width: 100%;
  }

  .tree-window {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 2px var(--space-2);
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
    box-sizing: border-box;
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
    display: inline-flex;
    align-items: center;
    color: var(--color-text-dim);
  }

  .icon.dir {
    color: var(--color-accent);
  }

  .browser-help {
    font-size: var(--text-xs);
    color: var(--color-text-dim);
    line-height: 1.5;
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
