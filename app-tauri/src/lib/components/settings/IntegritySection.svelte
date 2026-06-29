<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { profilesState } from '$lib/stores/profiles.svelte';
  import FieldHelp from '$lib/components/FieldHelp.svelte';

  interface IntegrityEvent {
    id: string;
    timestamp: string;
    profile_id: string;
    mode: 'repository' | 'verify_data';
    outcome: 'success' | 'failure' | 'cancelled';
    duration_seconds: number;
    error_message?: string;
  }

  let latest = $state<IntegrityEvent | null>(null);
  let running = $state(false);
  let result = $state('');
  let monthly = $state(false);

  async function refresh() {
    latest = await invoke<IntegrityEvent | null>('latest_integrity_check');
    monthly = profilesState.active?.integrity_schedule?.enabled ?? false;
  }

  async function run(verifyData: boolean) {
    running = true;
    result = verifyData
      ? 'Verifying repository metadata and all archived data…'
      : 'Checking repository metadata…';
    try {
      latest = await invoke<IntegrityEvent>('check_repository', { verifyData });
      result = latest.outcome === 'success'
        ? 'Repository check completed successfully.'
        : `Repository check found warnings: ${latest.error_message ?? 'Review Borg diagnostics.'}`;
    } catch (error) {
      if (String(error).includes('operation cancelled')) {
        result = 'Repository check cancelled.';
      } else {
        result = `Repository check failed: ${error}`;
      }
      latest = await invoke<IntegrityEvent | null>('latest_integrity_check');
    } finally {
      running = false;
    }
  }

  async function cancel() {
    await invoke('cancel_repository_check');
  }

  async function toggleMonthly() {
    const desired = monthly;
    try {
      await invoke('set_monthly_integrity_check', { enabled: monthly });
      await profilesState.load();
      result = monthly
        ? 'Monthly metadata check scheduled for the first day of each month.'
        : 'Monthly metadata check disabled.';
    } catch (error) {
      monthly = !desired;
      result = `Could not update integrity schedule: ${error}`;
    }
  }

  onMount(() => {
    refresh().catch((error) => (result = `Could not load integrity status: ${error}`));
  });
</script>

<fieldset class="form-group">
  <legend>Repository integrity</legend>
  <FieldHelp text="Metadata checks detect repository damage without modifying it. Full data verification reads every archived chunk and can take many hours. BorgUI never runs repair automatically." />

  {#if latest}
    <p class:failure={latest.outcome === 'failure'}>
      Latest: {latest.outcome.replace('_', ' ')} ·
      {latest.mode === 'verify_data' ? 'full data verification' : 'metadata'} ·
      {new Date(latest.timestamp).toLocaleString()}
    </p>
    {#if latest.error_message}<p class="failure">{latest.error_message}</p>{/if}
  {:else}
    <p>No integrity checks recorded for this profile.</p>
  {/if}

  <div class="actions">
    <button class="btn btn-primary" type="button" disabled={running} onclick={() => run(false)}>
      Check metadata
    </button>
    <button class="btn btn-secondary" type="button" disabled={running} onclick={() => run(true)}>
      Verify all data
    </button>
    {#if running}
      <button class="btn btn-secondary" type="button" onclick={cancel}>Cancel</button>
    {/if}
  </div>

  <label class="monthly">
    <input type="checkbox" bind:checked={monthly} onchange={toggleMonthly} />
    Run a metadata check monthly
  </label>
  {#if result}<p>{result}</p>{/if}
</fieldset>

<style>
  .actions { display: flex; gap: var(--space-2); flex-wrap: wrap; margin: var(--space-4) 0; }
  .monthly { display: flex; gap: var(--space-2); align-items: center; }
  p { color: var(--color-text-muted); font-size: var(--text-sm); overflow-wrap: anywhere; }
  .failure { color: var(--color-error); }
</style>
