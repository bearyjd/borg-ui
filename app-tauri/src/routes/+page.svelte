<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';

  let borgVersion = $state('checking...');
  let error = $state('');

  onMount(async () => {
    try {
      borgVersion = await invoke('get_borg_version');
    } catch (e) {
      error = `borg not found: ${e}`;
      borgVersion = 'not available';
    }
  });
</script>

<div class="dashboard">
  <header class="page-header">
    <h1>Dashboard</h1>
    <p class="subtitle">Backup status overview</p>
  </header>

  <div class="status-grid">
    <div class="status-card">
      <div class="card-label">Borg Engine</div>
      <div class="card-value" class:error={!!error}>
        {borgVersion}
      </div>
      {#if error}
        <div class="card-detail error">{error}</div>
      {/if}
    </div>

    <div class="status-card">
      <div class="card-label">Last Backup</div>
      <div class="card-value dimmed">No backups yet</div>
    </div>

    <div class="status-card">
      <div class="card-label">Repository</div>
      <div class="card-value dimmed">Not connected</div>
      <a href="/settings" class="card-action">Configure →</a>
    </div>

    <div class="status-card">
      <div class="card-label">Next Scheduled</div>
      <div class="card-value dimmed">Not scheduled</div>
    </div>
  </div>

  <section class="recent-section">
    <h2>Recent Activity</h2>
    <div class="empty-state">
      <p>No backup activity yet. <a href="/backup">Create your first backup</a> to get started.</p>
    </div>
  </section>
</div>

<style>
  .dashboard {
    max-width: 800px;
  }

  .page-header {
    margin-bottom: var(--space-8);
  }

  .page-header h1 {
    font-size: var(--text-3xl);
    font-weight: 700;
    letter-spacing: -0.03em;
    line-height: 1.1;
  }

  .subtitle {
    color: var(--color-text-muted);
    margin-top: var(--space-1);
  }

  .status-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: var(--space-4);
    margin-bottom: var(--space-8);
  }

  .status-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-lg);
    padding: var(--space-4) var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .card-label {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--color-text-dim);
    font-weight: 600;
  }

  .card-value {
    font-size: var(--text-lg);
    font-weight: 600;
    font-family: var(--font-mono);
  }

  .card-value.dimmed {
    color: var(--color-text-dim);
    font-weight: 400;
  }

  .card-value.error {
    color: var(--color-danger);
  }

  .card-detail.error {
    font-size: var(--text-xs);
    color: var(--color-danger);
  }

  .card-action {
    font-size: var(--text-sm);
    color: var(--color-accent);
    margin-top: var(--space-1);
  }

  .card-action:hover {
    color: var(--color-accent-hover);
  }

  .recent-section h2 {
    font-size: var(--text-xl);
    font-weight: 600;
    margin-bottom: var(--space-4);
  }

  .empty-state {
    background: var(--color-surface);
    border: 1px dashed var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-8);
    text-align: center;
    color: var(--color-text-muted);
  }
</style>
