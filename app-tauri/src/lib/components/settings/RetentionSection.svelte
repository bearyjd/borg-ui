<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { retentionState, type RetentionConfig } from '$lib/stores/retention.svelte';
  import { profilesState } from '$lib/stores/profiles.svelte';
  import type { RepoConfig } from '$lib/stores/repo.svelte';
  import FieldHelp from '$lib/components/FieldHelp.svelte';

  interface Props {
    getRepo: () => RepoConfig;
    repoConfigured: boolean;
  }

  let { getRepo, repoConfigured }: Props = $props();

  let keepHourly = $state<number | null>(null);
  let keepDaily = $state<number | null>(7);
  let keepWeekly = $state<number | null>(4);
  let keepMonthly = $state<number | null>(6);
  let keepYearly = $state<number | null>(null);
  let retentionSaving = $state(false);
  let retentionPruning = $state(false);
  let retentionResult = $state('');

  function currentRetention(): RetentionConfig {
    return {
      keep_hourly: keepHourly,
      keep_daily: keepDaily,
      keep_weekly: keepWeekly,
      keep_monthly: keepMonthly,
      keep_yearly: keepYearly,
    };
  }

  async function saveRetention() {
    retentionSaving = true;
    retentionResult = '';
    try {
      await retentionState.save(currentRetention());
      retentionResult = 'Retention policy saved.';
    } catch (e) {
      retentionResult = `Save failed: ${e}`;
    } finally {
      retentionSaving = false;
    }
  }

  async function runPrune() {
    retentionPruning = true;
    retentionResult = '';
    try {
      const warnings = await invoke<string[]>('prune_repo', {
        repo: getRepo(),
        retention: currentRetention(),
      });
      retentionResult =
        warnings.length > 0
          ? `Prune completed with ${warnings.length} warning${warnings.length === 1 ? '' : 's'}: ${warnings.join('; ')}`
          : 'Prune completed successfully.';
    } catch (e) {
      retentionResult = `Prune failed: ${e}`;
    } finally {
      retentionPruning = false;
    }
  }

  onMount(async () => {
    try {
      await retentionState.load();
      if (retentionState.config) {
        keepHourly = retentionState.config.keep_hourly;
        keepDaily = retentionState.config.keep_daily;
        keepWeekly = retentionState.config.keep_weekly;
        keepMonthly = retentionState.config.keep_monthly;
        keepYearly = retentionState.config.keep_yearly;
      }
    } catch {
      // No retention config yet
    }
  });

  let lastActiveId = $state<string | null>(profilesState.activeId);
  $effect(() => {
    const id = profilesState.activeId;
    if (id === lastActiveId) return;
    lastActiveId = id;

    if (retentionState.config) {
      keepHourly = retentionState.config.keep_hourly;
      keepDaily = retentionState.config.keep_daily;
      keepWeekly = retentionState.config.keep_weekly;
      keepMonthly = retentionState.config.keep_monthly;
      keepYearly = retentionState.config.keep_yearly;
    } else {
      keepHourly = null;
      keepDaily = 7;
      keepWeekly = 4;
      keepMonthly = 6;
      keepYearly = null;
    }
  });
</script>

<form class="settings-form" onsubmit={(e) => { e.preventDefault(); saveRetention(); }}>
  <fieldset class="form-group">
    <legend>Retention Policy</legend>
    <FieldHelp text="Old backups add up over time. This policy thins them out automatically: it keeps a recent few of each kind and deletes (prunes) the older ones that fall outside the rules. Leave a box empty to keep unlimited backups for that period." />
    <FieldHelp text="A common, comfortable setup: keep 7 daily + 4 weekly + 6 monthly. That's roughly six months of history, automatically thinned so it never balloons." />
    <FieldHelp text='"Run Prune Now" applies these rules immediately. Saving the policy makes scheduled backups apply it for you.' />

    <div class="field-row">
      <div class="field field-sm">
        <label for="keep-hourly">Keep hourly</label>
        <input id="keep-hourly" type="number" min="0" placeholder="—" bind:value={keepHourly} />
      </div>
      <div class="field field-sm">
        <label for="keep-daily">Keep daily</label>
        <input id="keep-daily" type="number" min="0" placeholder="—" bind:value={keepDaily} />
      </div>
      <div class="field field-sm">
        <label for="keep-weekly">Keep weekly</label>
        <input id="keep-weekly" type="number" min="0" placeholder="—" bind:value={keepWeekly} />
      </div>
    </div>
    <div class="field-row">
      <div class="field field-sm">
        <label for="keep-monthly">Keep monthly</label>
        <input id="keep-monthly" type="number" min="0" placeholder="—" bind:value={keepMonthly} />
      </div>
      <div class="field field-sm">
        <label for="keep-yearly">Keep yearly</label>
        <input id="keep-yearly" type="number" min="0" placeholder="—" bind:value={keepYearly} />
      </div>
    </div>

    <div class="form-actions">
      <button type="submit" class="btn btn-secondary" disabled={retentionSaving || retentionPruning}>
        {retentionSaving ? 'Saving...' : 'Save Policy'}
      </button>
      <button type="button" class="btn btn-primary" onclick={runPrune} disabled={retentionPruning || retentionSaving || !repoConfigured}>
        {retentionPruning ? 'Pruning...' : 'Run Prune Now'}
      </button>
    </div>

    {#if retentionResult}
      <div class="test-result" class:success={retentionResult.includes('success') || retentionResult.includes('saved')} class:error={retentionResult.includes('failed')}>
        {retentionResult}
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
</style>
