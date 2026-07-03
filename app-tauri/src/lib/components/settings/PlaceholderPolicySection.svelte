<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { profilesState, type PlaceholderPolicy } from '$lib/stores/profiles.svelte';
  import FieldHelp from '$lib/components/FieldHelp.svelte';

  let policy = $state<PlaceholderPolicy>({
    mode: 'warn_and_skip',
    minimum_free_space_reserve: 10 * 1024 * 1024 * 1024,
  });
  let reserveGiB = $state(10);
  let result = $state('');

  onMount(async () => {
    policy = await invoke('load_placeholder_policy');
    reserveGiB = Math.max(1, Math.round(policy.minimum_free_space_reserve / 1024 ** 3));
  });

  async function save() {
    policy.minimum_free_space_reserve = reserveGiB * 1024 ** 3;
    try {
      await invoke('save_placeholder_policy', { policy });
      await profilesState.load();
      result = 'Cloud placeholder policy saved.';
    } catch (error) {
      result = `Could not save placeholder policy: ${error}`;
    }
  }
</script>

<fieldset class="form-group">
  <legend>Cloud-only files</legend>
  <FieldHelp text="Windows cloud placeholders are not file content. BorgUI detects them before creating a VSS snapshot and never silently reports placeholder metadata as backed up data." />
  <label class="field">
    <span>When cloud-only files are found</span>
    <select bind:value={policy.mode}>
      <option value="warn_and_skip">Warn and skip exact placeholder files</option>
      <option value="fail">Fail the backup</option>
      <option value="materialize">Download files before snapshot</option>
    </select>
  </label>
  {#if policy.mode === 'materialize'}
    <label class="field">
      <span>Minimum free-space reserve (GiB)</span>
      <input type="number" min="1" bind:value={reserveGiB} />
    </label>
    <p>Files are downloaded sequentially and hydration can be cancelled. VSS starts only after all selected placeholders are readable.</p>
  {/if}
  <button class="btn btn-primary" onclick={save}>Save placeholder policy</button>
  {#if result}<p>{result}</p>{/if}
</fieldset>

<style>
  .form-group { display: grid; gap: var(--space-3); }
  .field { display: flex; flex-direction: column; gap: var(--space-1); color: var(--color-text-muted); }
  input, select { padding: var(--space-2) var(--space-3); border: 1px solid var(--color-border); border-radius: var(--radius-md); background: var(--color-bg); color: var(--color-text); }
  p { color: var(--color-text-muted); }
</style>
