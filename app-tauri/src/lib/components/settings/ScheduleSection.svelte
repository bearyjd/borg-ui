<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { scheduleState, nextRun, type ScheduleConfig } from '$lib/stores/schedule.svelte';
  import { profilesState } from '$lib/stores/profiles.svelte';
  import FieldHelp from '$lib/components/FieldHelp.svelte';

  let scheduleEnabled = $state(false);
  let scheduleType = $state<'hourly' | 'daily'>('daily');
  let scheduleHour = $state(2);
  let scheduleMinute = $state(0);
  let skipMeteredNetworks = $state(false);
  let scheduleSaving = $state(false);
  let scheduleResult = $state('');
  let taskDiagnostic = $state('');

  let scheduleNextRunLabel = $derived.by(() => {
    if (!scheduleEnabled) return '';
    const schedule = scheduleType === 'hourly'
      ? { type: 'hourly' as const }
      : { type: 'daily' as const, hour: scheduleHour, minute: scheduleMinute };
    const next = nextRun({
      enabled: true,
      schedule,
      skip_metered_networks: false,
    });
    if (!next) return '';
    return next.toLocaleString(undefined, {
      weekday: 'short',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  });

  async function saveSchedule() {
    scheduleSaving = true;
    scheduleResult = '';
    try {
      const schedule = scheduleType === 'hourly'
        ? { type: 'hourly' as const }
        : { type: 'daily' as const, hour: scheduleHour, minute: scheduleMinute };
      const config: ScheduleConfig = {
        enabled: scheduleEnabled,
        schedule,
        skip_metered_networks: skipMeteredNetworks,
      };
      await scheduleState.save(config);
      await loadTaskDiagnostic();
      scheduleResult = scheduleEnabled ? 'Schedule saved and activated.' : 'Schedule disabled.';
    } catch (e) {
      scheduleResult = `Schedule save failed: ${e}`;
    } finally {
      scheduleSaving = false;
    }
  }

  async function loadTaskDiagnostic() {
    const status = await invoke<{
      task_registered: boolean;
      last_attempt: { outcome: string; attempt: number } | null;
    }>('scheduled_backup_status');
    taskDiagnostic = status.task_registered
      ? `Windows task registered${status.last_attempt ? `; last run ended ${status.last_attempt.outcome} on attempt ${status.last_attempt.attempt}` : ''}.`
      : 'Windows task is not registered.';
  }

  onMount(async () => {
    try {
      await scheduleState.load();
      await loadTaskDiagnostic();
      if (scheduleState.config) {
        scheduleEnabled = scheduleState.config.enabled;
        skipMeteredNetworks = scheduleState.config.skip_metered_networks ?? false;
        if (scheduleState.config.schedule.type === 'hourly') {
          scheduleType = 'hourly';
        } else {
          scheduleType = 'daily';
          scheduleHour = scheduleState.config.schedule.hour;
          scheduleMinute = scheduleState.config.schedule.minute;
        }
      }
    } catch {
      // No schedule config yet
    }
  });

  let lastActiveId = $state<string | null>(profilesState.activeId);
  $effect(() => {
    const id = profilesState.activeId;
    if (id === lastActiveId) return;
    lastActiveId = id;
    loadTaskDiagnostic().catch(() => {
      taskDiagnostic = '';
    });

    if (scheduleState.config) {
      scheduleEnabled = scheduleState.config.enabled;
      skipMeteredNetworks = scheduleState.config.skip_metered_networks ?? false;
      if (scheduleState.config.schedule.type === 'hourly') {
        scheduleType = 'hourly';
      } else {
        scheduleType = 'daily';
        scheduleHour = scheduleState.config.schedule.hour;
        scheduleMinute = scheduleState.config.schedule.minute;
      }
    } else {
      scheduleEnabled = false;
      scheduleType = 'daily';
      scheduleHour = 2;
      scheduleMinute = 0;
      skipMeteredNetworks = false;
    }
  });
</script>

<form class="settings-form" onsubmit={(e) => { e.preventDefault(); saveSchedule(); }}>
  <fieldset class="form-group">
    <legend>Scheduled Backups</legend>
    <FieldHelp text="Let BorgUI back up on its own using Windows Task Scheduler. Because Windows runs it, scheduled backups happen even when this app is closed — you just need BorgUI installed on the PC, not open. Choose “Every hour” for frequent protection, or “Daily” to run once at a set time (a quiet hour like 2:00 AM is a good choice)." />

    <div class="field">
      <label class="toggle-row">
        <input type="checkbox" bind:checked={scheduleEnabled} />
        <span>Enable scheduled backups</span>
      </label>
    </div>

    {#if scheduleEnabled}
      <div class="field">
        <label for="schedule-type">Frequency</label>
        <select id="schedule-type" bind:value={scheduleType}>
          <option value="hourly">Every hour</option>
          <option value="daily">Daily</option>
        </select>
      </div>

      {#if scheduleNextRunLabel}
        <div class="next-run">
          <span class="next-run-label">Next run</span>
          <span class="next-run-value">{scheduleNextRunLabel}</span>
        </div>
      {/if}

      {#if scheduleType === 'daily'}
        <div class="field-row">
          <div class="field field-sm">
            <label for="schedule-hour">Hour</label>
            <input id="schedule-hour" type="number" min="0" max="23" bind:value={scheduleHour} />
          </div>
          <div class="field field-sm">
            <label for="schedule-minute">Minute</label>
            <input id="schedule-minute" type="number" min="0" max="59" bind:value={scheduleMinute} />
          </div>
        </div>
      {/if}

      <p class="selection-note">
        Scheduled and manual backups use the protected folders saved on the
        <a href="/backup">Backup page</a>.
      </p>

      <div class="field">
        <label class="toggle-row">
          <input type="checkbox" bind:checked={skipMeteredNetworks} />
          <span>Skip scheduled backups on metered networks</span>
        </label>
        <FieldHelp text="When enabled, scheduled backups do not run while Windows marks the active connection as metered, roaming, or near/over a data limit. Manual backups still run when you start them." />
      </div>
    {/if}

    <div class="form-actions">
      <button type="submit" class="btn btn-primary" disabled={scheduleSaving}>
        {scheduleSaving ? 'Saving...' : 'Save Schedule'}
      </button>
    </div>

    {#if scheduleResult}
      <div class="test-result" class:success={scheduleResult.includes('saved') || scheduleResult.includes('disabled')} class:error={scheduleResult.includes('failed')}>
        {scheduleResult}
      </div>
    {/if}
    {#if scheduleEnabled && taskDiagnostic}<p class="task-diagnostic">{taskDiagnostic}</p>{/if}
  </fieldset>
</form>

<style>
  .settings-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }

  .form-group {
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-lg);
    padding: var(--space-6);
    background: var(--color-surface);
  }

  .form-group legend {
    font-size: var(--text-sm);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--color-text-muted);
    padding: 0 var(--space-2);
  }

  .next-run {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    margin-top: var(--space-4);
    padding: var(--space-2) var(--space-3);
    background: var(--color-bg);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-sm);
  }

  .next-run-label {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--color-text-dim);
    font-weight: 600;
  }

  .next-run-value {
    font-size: var(--text-sm);
    font-family: var(--font-mono);
    color: var(--color-accent);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin-top: var(--space-4);
  }

  .field label {
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-text-muted);
  }

  .field input {
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: var(--space-2) var(--space-3);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    transition: border-color var(--duration-fast) var(--ease-out);
  }

  .field input:focus {
    outline: none;
    border-color: var(--color-accent);
  }

  .field input::placeholder {
    color: var(--color-text-dim);
  }

  .field-row {
    display: flex;
    gap: var(--space-4);
  }

  .field-row .field {
    flex: 1;
  }

  .field-row .field-sm {
    flex: 0 0 100px;
  }

  .form-actions {
    display: flex;
    gap: var(--space-3);
    margin-top: var(--space-6);
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

  .btn-primary {
    background: var(--color-accent);
    color: var(--color-on-accent);
  }

  .btn-primary:hover:not(:disabled) {
    background: var(--color-accent-hover);
  }

  .test-result {
    margin-top: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
    background: var(--color-surface-hover);
    color: var(--color-text-muted);
  }

  .test-result.success {
    background: var(--color-success-muted);
    color: var(--color-success);
  }

  .test-result.error {
    background: var(--color-danger-muted);
    color: var(--color-danger);
  }

  select {
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: var(--space-2) var(--space-3);
    color: var(--color-text);
    font-size: var(--text-sm);
  }

  select:focus {
    outline: none;
    border-color: var(--color-accent);
  }

  .selection-note {
    color: var(--color-text-muted);
  }
</style>
