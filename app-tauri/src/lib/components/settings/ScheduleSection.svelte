<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import { scheduleState, nextRun, type ScheduleConfig } from '$lib/stores/schedule.svelte';
  import { profilesState } from '$lib/stores/profiles.svelte';
  import FieldHelp from '$lib/components/FieldHelp.svelte';

  const EXCLUDE_PRESETS = ['*.tmp', '*.cache', 'node_modules', '.git', 'target', '__pycache__', '.venv', 'dist', 'build'];

  let scheduleEnabled = $state(false);
  let scheduleType = $state<'hourly' | 'daily'>('daily');
  let scheduleHour = $state(2);
  let scheduleMinute = $state(0);
  let schedulePaths = $state<string[]>([]);
  let scheduleExcludes = $state<string[]>([]);
  let scheduleExcludeInput = $state('');
  let scheduleSaving = $state(false);
  let scheduleResult = $state('');

  let scheduleNextRunLabel = $derived.by(() => {
    if (!scheduleEnabled) return '';
    const schedule = scheduleType === 'hourly'
      ? { type: 'hourly' as const }
      : { type: 'daily' as const, hour: scheduleHour, minute: scheduleMinute };
    const next = nextRun({ enabled: true, source_paths: [], schedule, excludes: [] });
    if (!next) return '';
    return next.toLocaleString(undefined, {
      weekday: 'short',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  });

  function addScheduleExclude(pattern: string) {
    const trimmed = pattern.trim();
    if (trimmed && !scheduleExcludes.includes(trimmed)) {
      scheduleExcludes = [...scheduleExcludes, trimmed];
    }
    scheduleExcludeInput = '';
  }

  function removeScheduleExclude(index: number) {
    scheduleExcludes = scheduleExcludes.filter((_, i) => i !== index);
  }

  async function addScheduleFolder() {
    const selected = await open({ directory: true, multiple: false, title: 'Select folder for scheduled backup' });
    if (selected && !schedulePaths.includes(selected as string)) {
      schedulePaths = [...schedulePaths, selected as string];
    }
  }

  async function saveSchedule() {
    scheduleSaving = true;
    scheduleResult = '';
    try {
      const schedule = scheduleType === 'hourly'
        ? { type: 'hourly' as const }
        : { type: 'daily' as const, hour: scheduleHour, minute: scheduleMinute };
      const config: ScheduleConfig = {
        enabled: scheduleEnabled,
        source_paths: schedulePaths,
        schedule,
        excludes: scheduleExcludes,
      };
      await scheduleState.save(config);
      scheduleResult = scheduleEnabled ? 'Schedule saved and activated.' : 'Schedule disabled.';
    } catch (e) {
      scheduleResult = `Schedule save failed: ${e}`;
    } finally {
      scheduleSaving = false;
    }
  }

  onMount(async () => {
    try {
      await scheduleState.load();
      if (scheduleState.config) {
        scheduleEnabled = scheduleState.config.enabled;
        schedulePaths = [...scheduleState.config.source_paths];
        scheduleExcludes = [...(scheduleState.config.excludes ?? [])];
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

    if (scheduleState.config) {
      scheduleEnabled = scheduleState.config.enabled;
      schedulePaths = [...scheduleState.config.source_paths];
      scheduleExcludes = [...(scheduleState.config.excludes ?? [])];
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
      schedulePaths = [];
      scheduleExcludes = [];
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

      <div class="field">
        <span class="field-label">Source Folders</span>
        <div class="path-list">
          {#if schedulePaths.length === 0}
            <p class="empty-hint">No folders selected</p>
          {/if}
          {#each schedulePaths as path, i}
            <div class="path-item">
              <code>{path}</code>
              <button type="button" onclick={() => schedulePaths = schedulePaths.filter((_, idx) => idx !== i)}>✕</button>
            </div>
          {/each}
        </div>
        <button type="button" class="btn btn-secondary" onclick={addScheduleFolder}>
          + Add Folder
        </button>
      </div>

      <div class="field">
        <span class="field-label">Exclude Patterns</span>
        {#if scheduleExcludes.length > 0}
          <div class="chip-list">
            {#each scheduleExcludes as pattern, i}
              <span class="chip">
                <code>{pattern}</code>
                <button type="button" class="chip-remove" onclick={() => removeScheduleExclude(i)} aria-label="Remove pattern">✕</button>
              </span>
            {/each}
          </div>
        {/if}
        <div class="exclude-input-row">
          <input
            type="text"
            class="exclude-input"
            placeholder="e.g. *.log or node_modules"
            bind:value={scheduleExcludeInput}
            onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); addScheduleExclude(scheduleExcludeInput); } }}
          />
          <button type="button" class="btn btn-secondary" onclick={() => addScheduleExclude(scheduleExcludeInput)} disabled={!scheduleExcludeInput.trim()}>
            + Add
          </button>
        </div>
        <div class="preset-row">
          <span class="preset-label">Presets:</span>
          {#each EXCLUDE_PRESETS as preset}
            <button
              type="button"
              class="preset-chip"
              onclick={() => addScheduleExclude(preset)}
              disabled={scheduleExcludes.includes(preset)}
            >
              {preset}
            </button>
          {/each}
        </div>
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

  .field label,
  .field .field-label {
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

  .btn-secondary {
    background: var(--color-surface-hover);
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
  }

  .btn-secondary:hover:not(:disabled) {
    background: var(--color-surface-active);
    color: var(--color-text);
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

  .path-list {
    background: var(--color-bg);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    padding: var(--space-3);
    min-height: 60px;
  }

  .empty-hint {
    color: var(--color-text-dim);
    text-align: center;
    padding: var(--space-3);
    font-size: var(--text-sm);
  }

  .path-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-hover);
    border-radius: var(--radius-sm);
  }

  .path-item + .path-item {
    margin-top: var(--space-2);
  }

  .path-item code {
    font-size: var(--text-sm);
  }

  .path-item button {
    color: var(--color-text-dim);
    padding: var(--space-1);
  }

  .path-item button:hover {
    color: var(--color-danger);
  }

  .chip-list {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--color-surface-hover);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    padding: var(--space-1) var(--space-3);
    font-size: var(--text-sm);
  }

  .chip code {
    font-family: var(--font-mono);
  }

  .chip-remove {
    background: transparent;
    border: none;
    color: var(--color-text-dim);
    cursor: pointer;
    padding: 0 2px;
    font-size: var(--text-xs);
  }

  .chip-remove:hover:not(:disabled) {
    color: var(--color-danger);
  }

  .exclude-input-row {
    display: flex;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
  }

  .exclude-input {
    flex: 1;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-3);
    color: var(--color-text);
    font-size: var(--text-sm);
    font-family: var(--font-mono);
  }

  .exclude-input:focus {
    outline: none;
    border-color: var(--color-accent);
  }

  .preset-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }

  .preset-label {
    font-size: var(--text-xs);
    color: var(--color-text-dim);
    margin-right: var(--space-1);
  }

  .preset-chip {
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: 2px var(--space-2);
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    cursor: pointer;
    transition: all var(--duration-fast) var(--ease-out);
  }

  .preset-chip:hover:not(:disabled) {
    background: var(--color-surface-hover);
    color: var(--color-text);
    border-color: var(--color-text-muted);
  }

  .preset-chip:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
</style>
