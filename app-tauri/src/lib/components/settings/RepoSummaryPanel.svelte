<script lang="ts">
  import { formatBytes } from '$lib/format';
  import type { RepoSummary } from '$lib/repo-info';

  interface Props {
    summary: RepoSummary;
    /** Set when the repo was readable but the backup list wasn't. */
    warning: string;
  }

  let { summary, warning }: Props = $props();

  function formatArchiveTime(start: string): string {
    const parsed = new Date(start);
    return Number.isNaN(parsed.getTime()) ? start : parsed.toLocaleString();
  }
</script>

<section class="repo-summary" aria-label="Repository contents">
  <h3>Repository found</h3>
  <dl>
    <div class="summary-row">
      <dt>Encryption</dt>
      <dd>{summary.encryption}</dd>
    </div>
    <div class="summary-row">
      <dt>Original size</dt>
      <dd>{summary.totalSize === null ? 'N/A' : formatBytes(summary.totalSize)}</dd>
    </div>
    <div class="summary-row">
      <dt>Compressed size</dt>
      <dd>{summary.compressedSize === null ? 'N/A' : formatBytes(summary.compressedSize)}</dd>
    </div>
    <div class="summary-row">
      <dt>Deduplicated size</dt>
      <dd>{summary.dedupSize === null ? 'N/A' : formatBytes(summary.dedupSize)}</dd>
    </div>
    {#if !warning}
      <div class="summary-row">
        <dt>Backups</dt>
        <dd>{summary.archiveCount}</dd>
      </div>
      {#if summary.latestArchive}
        <div class="summary-row">
          <dt>Latest backup</dt>
          <dd>{summary.latestArchive.name} — {formatArchiveTime(summary.latestArchive.start)}</dd>
        </div>
      {/if}
    {/if}
  </dl>
  {#if warning}
    <p class="summary-warning" role="status">{warning}</p>
  {:else if summary.archiveCount === 0}
    <p class="summary-note">The repository is ready but has no backups yet. Head to the Backup page to run your first one.</p>
  {:else}
    <p class="summary-note">Browse and restore these backups from the Archives page.</p>
  {/if}
</section>

<style>
  .repo-summary {
    margin-top: var(--space-3);
    padding: var(--space-4);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    background: var(--color-bg);
  }

  .repo-summary h3 {
    margin: 0 0 var(--space-3);
    font-size: var(--text-sm);
    color: var(--color-success);
  }

  .repo-summary dl {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin: 0;
  }

  .summary-row {
    display: flex;
    gap: var(--space-3);
    font-size: var(--text-xs);
  }

  .summary-row dt {
    flex: 0 0 9rem;
    color: var(--color-text-dim);
  }

  .summary-row dd {
    margin: 0;
    color: var(--color-text);
    font-family: var(--font-mono);
    overflow-wrap: anywhere;
  }

  .summary-note {
    margin: var(--space-3) 0 0;
    font-size: var(--text-xs);
    color: var(--color-text-dim);
    line-height: 1.5;
  }

  .summary-warning {
    margin: var(--space-3) 0 0;
    font-size: var(--text-xs);
    color: var(--color-warning);
    line-height: 1.5;
    overflow-wrap: anywhere;
  }
</style>
