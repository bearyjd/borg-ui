<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import type { RepoConfig } from '$lib/stores/repo.svelte';
  import { formatBytes } from '$lib/format';

  interface Props {
    repo: RepoConfig;
    archiveA: string;
    archiveB: string;
    onClose: () => void;
  }

  type DiffStatus = 'added' | 'removed' | 'modified' | 'changed';

  interface DiffEntry {
    path: string;
    status: DiffStatus;
    added: number;
    removed: number;
  }

  let { repo, archiveA, archiveB, onClose }: Props = $props();

  let entries = $state<DiffEntry[]>([]);
  let loading = $state(true);
  let error = $state('');
  // Metadata-only changes (timestamps/mode/owner) are common noise between two
  // backups of the same tree; hide them by default so the meaningful content
  // changes stand out. The toggle brings them back.
  let showMetadata = $state(false);
  let closeBtn = $state<HTMLButtonElement | null>(null);

  // Order content changes first, metadata-only last; then alphabetically.
  const STATUS_ORDER: Record<DiffStatus, number> = {
    added: 0,
    removed: 1,
    modified: 2,
    changed: 3,
  };
  const STATUS_LABEL: Record<DiffStatus, string> = {
    added: 'Added',
    removed: 'Removed',
    modified: 'Modified',
    changed: 'Changed',
  };

  let visible = $derived(
    entries
      .filter((e) => showMetadata || e.status !== 'changed')
      .sort(
        (a, b) =>
          STATUS_ORDER[a.status] - STATUS_ORDER[b.status] || a.path.localeCompare(b.path),
      ),
  );
  let counts = $derived({
    added: entries.filter((e) => e.status === 'added').length,
    removed: entries.filter((e) => e.status === 'removed').length,
    modified: entries.filter((e) => e.status === 'modified').length,
    changed: entries.filter((e) => e.status === 'changed').length,
  });

  $effect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', handler);
    closeBtn?.focus();
    return () => window.removeEventListener('keydown', handler);
  });

  $effect(() => {
    void load();
  });

  async function load() {
    loading = true;
    error = '';
    try {
      entries = await invoke<DiffEntry[]>('diff_archives', {
        repo,
        archiveA,
        archiveB,
      });
    } catch (e) {
      error = `Failed to compare archives: ${e}`;
    } finally {
      loading = false;
    }
  }

  function delta(e: DiffEntry): string {
    if (e.status === 'added') return `+${formatBytes(e.added)}`;
    if (e.status === 'removed') return `−${formatBytes(e.removed)}`;
    if (e.status === 'modified') {
      const parts: string[] = [];
      if (e.added) parts.push(`+${formatBytes(e.added)}`);
      if (e.removed) parts.push(`−${formatBytes(e.removed)}`);
      return parts.join(' ');
    }
    return '';
  }
</script>

<div class="modal-backdrop" onclick={onClose} role="presentation">
  <div
    class="modal diff"
    onclick={(e) => e.stopPropagation()}
    onkeydown={() => {}}
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-labelledby="diff-title"
  >
    <header class="diff-header">
      <div>
        <h2 id="diff-title">Compare archives</h2>
        <div class="diff-pair">
          <code>{archiveA}</code>
          <span class="arrow" aria-hidden="true">→</span>
          <code>{archiveB}</code>
        </div>
      </div>
    </header>

    {#if loading}
      <div class="diff-state">Comparing archives…</div>
    {:else if error}
      <div class="error-banner">{error}</div>
    {:else if entries.length === 0}
      <div class="diff-state">No differences — these archives are identical.</div>
    {:else}
      <div class="diff-summary">
        <span class="chip added">{counts.added} added</span>
        <span class="chip removed">{counts.removed} removed</span>
        <span class="chip modified">{counts.modified} modified</span>
        {#if counts.changed > 0}
          <label class="meta-toggle">
            <input type="checkbox" bind:checked={showMetadata} />
            {counts.changed} metadata-only
          </label>
        {/if}
      </div>

      {#if visible.length === 0}
        <div class="diff-state">Only metadata changed. Tick "metadata-only" to see those entries.</div>
      {:else}
        <div class="diff-scroll">
          <ul class="diff-list">
            {#each visible as entry (entry.path)}
              <li class="diff-row">
                <span class="badge {entry.status}">{STATUS_LABEL[entry.status]}</span>
                <span class="path" title={entry.path}>{entry.path}</span>
                <span class="delta">{delta(entry)}</span>
              </li>
            {/each}
          </ul>
        </div>
      {/if}
    {/if}

    <footer class="diff-footer">
      <button bind:this={closeBtn} class="btn btn-secondary" onclick={onClose}>Close</button>
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

  .modal.diff {
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

  .diff-header h2 {
    font-size: var(--text-lg);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .diff-pair {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-1);
    flex-wrap: wrap;
  }

  .diff-pair code {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    background: var(--color-surface-hover);
    padding: 1px 6px;
    border-radius: var(--radius-sm);
  }

  .diff-pair .arrow {
    color: var(--color-text-dim);
  }

  .diff-state {
    padding: var(--space-6);
    text-align: center;
    color: var(--color-text-dim);
    font-size: var(--text-sm);
  }

  .diff-summary {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
    font-size: var(--text-sm);
  }

  .chip {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    font-weight: 600;
  }

  .chip.added {
    color: var(--color-success);
  }

  .chip.removed {
    color: var(--color-danger);
  }

  .chip.modified {
    color: var(--color-warning);
  }

  .meta-toggle {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    cursor: pointer;
    margin-left: auto;
  }

  .diff-scroll {
    flex: 1;
    overflow-y: auto;
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-1);
    background: var(--color-surface-hover);
    min-height: 200px;
  }

  .diff-list {
    list-style: none;
    padding: 0;
    margin: 0;
  }

  .diff-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 2px var(--space-2);
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
  }

  .diff-row:hover {
    background: var(--color-surface-active);
  }

  .badge {
    flex-shrink: 0;
    width: 4.5rem;
    text-align: center;
    font-size: 0.65rem;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 1px 0;
    border-radius: var(--radius-sm);
  }

  .badge.added {
    background: var(--color-success-muted);
    color: var(--color-success);
  }

  .badge.removed {
    background: var(--color-danger-muted);
    color: var(--color-danger);
  }

  .badge.modified {
    background: var(--color-warning-muted);
    color: var(--color-warning);
  }

  .badge.changed {
    background: var(--color-surface-active);
    color: var(--color-text-dim);
  }

  .path {
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .delta {
    flex-shrink: 0;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-dim);
  }

  .diff-footer {
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
