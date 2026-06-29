<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { repoState, describeRepo } from '$lib/stores/repo.svelte';
  import { scheduleState, describeSchedule, nextRun } from '$lib/stores/schedule.svelte';
  import { historyState, type BackupEvent } from '$lib/stores/history.svelte';
  import { formatBytes } from '$lib/format';

  let borgVersion = $state('checking...');
  let borgError = $state('');
  let latestIntegrity = $state<{
    timestamp: string;
    mode: string;
    outcome: string;
  } | null>(null);
  let scheduledStatus = $state<{
    missed: boolean;
    task_registered: boolean;
    last_attempt: { timestamp: string; outcome: string; attempt: number } | null;
  } | null>(null);

  let repoHost = $derived(repoState.config ? describeRepo(repoState.config) : '');
  let hasRepo = $derived(repoState.hasRepo);
  let schedule = $derived(scheduleState.config);
  let scheduleLabel = $derived(
    schedule && schedule.enabled ? describeSchedule(schedule) : ''
  );
  let scheduleNext = $derived(
    schedule && schedule.enabled ? nextRun(schedule) : null
  );
  let events = $derived(historyState.events);
  let lastBackup = $derived(
    [...events]
      .reverse()
      .find((e) => e.kind === 'backup' && e.outcome === 'success')
  );
  let recent = $derived([...events].slice(-10).reverse());

  function formatRelative(iso: string): string {
    const diffMs = Date.now() - new Date(iso).getTime();
    const sec = Math.round(diffMs / 1000);
    if (sec < 60) return `${sec}s ago`;
    const min = Math.round(sec / 60);
    if (min < 60) return `${min}m ago`;
    const hr = Math.round(min / 60);
    if (hr < 24) return `${hr}h ago`;
    const day = Math.round(hr / 24);
    return `${day}d ago`;
  }

  function formatDuration(sec: number): string {
    if (sec < 60) return `${sec}s`;
    const min = Math.floor(sec / 60);
    const remSec = sec % 60;
    if (min < 60) return remSec === 0 ? `${min}m` : `${min}m ${remSec}s`;
    const hr = Math.floor(min / 60);
    const remMin = min % 60;
    return remMin === 0 ? `${hr}h` : `${hr}h ${remMin}m`;
  }

  function formatNextRun(date: Date): string {
    return date.toLocaleString(undefined, {
      weekday: 'short',
      hour: '2-digit',
      minute: '2-digit',
    });
  }

  onMount(async () => {
    try {
      borgVersion = await invoke('get_borg_version');
    } catch (e) {
      borgError = `borg not found: ${e}`;
      borgVersion = 'not available';
    }
    try {
      await historyState.load();
    } catch (e) {
      console.warn('Failed to load history:', e);
    }
    try {
      await scheduleState.load();
    } catch (e) {
      console.warn('Failed to load schedule:', e);
    }
    try {
      latestIntegrity = await invoke('latest_integrity_check');
    } catch (e) {
      console.warn('Failed to load integrity status:', e);
    }
    try {
      scheduledStatus = await invoke('scheduled_backup_status');
    } catch (e) {
      console.warn('Failed to load scheduled backup status:', e);
    }
  });
</script>

<div class="dashboard">
  <header class="page-header">
    <h1>Dashboard</h1>
    <p class="subtitle">Backup status overview</p>
  </header>

  {#if scheduledStatus?.missed}
    <div class="schedule-warning">
      A scheduled backup appears to have been missed. The last attempt was
      {scheduledStatus.last_attempt ? formatRelative(scheduledStatus.last_attempt.timestamp) : 'not recorded'}.
      {#if !scheduledStatus.task_registered} Windows Task Scheduler does not report the BorgUI backup task as registered.{/if}
      Review the schedule in <a href="/settings">Settings</a>.
    </div>
  {/if}

  <div class="status-grid">
    <div class="status-card">
      <div class="card-label">Borg Engine</div>
      <div class="card-value" class:error={!!borgError}>
        {borgVersion}
      </div>
      {#if borgError}
        <div class="card-detail error">{borgError}</div>
      {/if}
    </div>

    <div class="status-card">
      <div class="card-label">Last Backup</div>
      {#if lastBackup}
        <div class="card-value">{formatRelative(lastBackup.timestamp)}</div>
        <div class="card-detail">
          {lastBackup.file_count?.toLocaleString() ?? '—'} files
          {#if lastBackup.original_size}· {formatBytes(lastBackup.original_size)}{/if}
          · {formatDuration(lastBackup.duration_seconds)}
        </div>
      {:else}
        <div class="card-value dimmed">No backups yet</div>
      {/if}
    </div>

    <div class="status-card">
      <div class="card-label">Repository</div>
      {#if hasRepo}
        <div class="card-value connected">{repoHost}</div>
      {:else}
        <div class="card-value dimmed">Not connected</div>
        <a href="/settings" class="card-action">Configure →</a>
      {/if}
    </div>

    <div class="status-card">
      <div class="card-label">Next Scheduled</div>
      {#if scheduleLabel}
        <div class="card-value">{scheduleLabel}</div>
        {#if scheduleNext}
          <div class="card-detail">Next run {formatNextRun(scheduleNext)}</div>
        {/if}
      {:else}
        <div class="card-value dimmed">Not scheduled</div>
        <a href="/settings" class="card-action">Set up →</a>
      {/if}
    </div>

    <div class="status-card">
      <div class="card-label">Repository Integrity</div>
      {#if latestIntegrity}
        <div class="card-value" class:error={latestIntegrity.outcome === 'failure'}>
          {latestIntegrity.outcome}
        </div>
        <div class="card-detail">
          {latestIntegrity.mode === 'verify_data' ? 'Full data' : 'Metadata'} ·
          {formatRelative(latestIntegrity.timestamp)}
        </div>
      {:else}
        <div class="card-value dimmed">Not checked</div>
        <a href="/settings" class="card-action">Run check →</a>
      {/if}
    </div>
  </div>

  {#if !hasRepo}
    <section class="first-run">
      <h2>Let's get you set up</h2>
      <p class="first-run-intro">Four quick steps and your PC is protected. Do them in order.</p>
      <ol class="checklist">
        <li>
          <span class="step-num">1</span>
          <div class="step-body">
            <a href="/settings">Choose a backup destination</a>
            <p>A backup server (SSH) or a local folder, USB drive, or network share.</p>
          </div>
        </li>
        <li>
          <span class="step-num">2</span>
          <div class="step-body">
            <a href="/settings">Initialize or connect the repository</a>
            <p>Create a fresh repository for a new destination, or connect to one you already have.</p>
          </div>
        </li>
        <li>
          <span class="step-num">3</span>
          <div class="step-body">
            <a href="/settings">Set your passphrase</a>
            <p>The password that unlocks your encrypted backups. Keep it somewhere safe.</p>
          </div>
        </li>
        <li>
          <span class="step-num">4</span>
          <div class="step-body">
            <a href="/backup">Run your first backup</a>
            <p>Pick the folders you care about and let BorgUI do the rest.</p>
          </div>
        </li>
      </ol>
    </section>
  {/if}

  <section class="recent-section">
    <h2>Recent Activity</h2>
    {#if recent.length === 0}
      <div class="empty-state">
        <p>No backup activity yet. <a href="/backup">Create your first backup</a> to get started.</p>
      </div>
    {:else}
      <ul class="event-list">
        {#each recent as event (event.id)}
          <li class="event-row" class:failure={event.outcome === 'failure'}>
            <span class="event-dot" class:success={event.outcome === 'success'} aria-hidden="true"></span>
            <div class="event-main">
              <div class="event-title">
                <span class="event-kind">{event.kind}</span>
                <code class="event-archive">{event.archive_name}</code>
              </div>
              {#if event.outcome === 'failure' && event.error_message}
                <div class="event-error">{event.error_message}</div>
              {:else}
                <div class="event-detail">
                  {#if event.file_count}{event.file_count.toLocaleString()} files · {/if}
                  {#if event.original_size}{formatBytes(event.original_size)} · {/if}
                  {formatDuration(event.duration_seconds)}
                </div>
              {/if}
            </div>
            <time class="event-time" datetime={event.timestamp}>{formatRelative(event.timestamp)}</time>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</div>

<style>
  .dashboard {
    max-width: 800px;
  }

  .page-header {
    margin-bottom: var(--space-8);
  }

  .schedule-warning {
    border: 1px solid var(--color-warning);
    border-radius: var(--radius-md);
    padding: var(--space-3);
    margin-bottom: var(--space-4);
    color: var(--color-text-muted);
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

  .card-value.connected {
    color: var(--color-success);
    font-size: var(--text-sm);
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

  .first-run {
    background: var(--color-surface);
    border: 1px solid var(--color-accent-muted);
    border-radius: var(--radius-lg);
    padding: var(--space-6);
    margin-bottom: var(--space-8);
  }

  .first-run h2 {
    font-size: var(--text-xl);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .first-run-intro {
    margin-top: var(--space-1);
    color: var(--color-text-muted);
    font-size: var(--text-sm);
  }

  .checklist {
    list-style: none;
    margin-top: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .checklist li {
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
  }

  .step-num {
    flex-shrink: 0;
    width: 24px;
    height: 24px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--color-accent-muted);
    color: var(--color-accent);
    font-size: var(--text-xs);
    font-weight: 700;
  }

  .step-body a {
    font-size: var(--text-sm);
    font-weight: 600;
  }

  .step-body p {
    margin-top: 2px;
    font-size: var(--text-xs);
    color: var(--color-text-dim);
    line-height: 1.5;
  }

  .empty-state {
    background: var(--color-surface);
    border: 1px dashed var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-8);
    text-align: center;
    color: var(--color-text-muted);
  }

  .card-detail {
    font-size: var(--text-xs);
    color: var(--color-text-dim);
    margin-top: var(--space-1);
  }

  .event-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .event-row {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
  }

  .event-dot {
    flex-shrink: 0;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--color-danger);
    margin-top: 6px;
  }

  .event-dot.success {
    background: var(--color-success);
  }

  .event-main {
    flex: 1;
    min-width: 0;
  }

  .event-title {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }

  .event-kind {
    text-transform: uppercase;
    font-size: var(--text-xs);
    letter-spacing: 0.06em;
    color: var(--color-text-dim);
    font-weight: 600;
  }

  .event-archive {
    font-family: var(--font-mono);
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .event-detail {
    font-size: var(--text-xs);
    color: var(--color-text-dim);
    margin-top: 2px;
  }

  .event-error {
    font-size: var(--text-xs);
    color: var(--color-danger);
    margin-top: 2px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .event-time {
    flex-shrink: 0;
    font-size: var(--text-xs);
    color: var(--color-text-dim);
    margin-top: 2px;
  }
</style>
