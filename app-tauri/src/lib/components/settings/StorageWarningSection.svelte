<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { profilesState } from '$lib/stores/profiles.svelte';

  let saving = $state(false);
  let message = $state('');
  let freeSpaceGiB = $state(20);
  let warningDays = $state(30);

  $effect(() => {
    const profile = profilesState.active;
    if (profile) {
      freeSpaceGiB = Math.round(profile.storage_warnings.minimum_free_space_bytes / 1073741824);
      warningDays = profile.storage_warnings.capacity_warning_days;
    }
  });

  async function save() {
    saving = true;
    message = '';
    try {
      await invoke('save_storage_warnings', {
        thresholds: {
          minimum_free_space_bytes: freeSpaceGiB * 1073741824,
          capacity_warning_days: warningDays,
        },
      });
      await profilesState.load();
      message = 'Storage warnings saved.';
    } catch (error) {
      message = String(error);
    } finally {
      saving = false;
    }
  }
</script>

<section class="settings-section">
  <h2>Storage warnings</h2>
  <p>Warnings are advisory. BorgUI never deletes archives automatically to recover capacity.</p>
  <label>
    Minimum free space (GiB)
    <input type="number" min="1" bind:value={freeSpaceGiB} />
  </label>
  <label>
    Warn when projected capacity is within (days)
    <input type="number" min="1" bind:value={warningDays} />
  </label>
  <button onclick={save} disabled={saving}>{saving ? 'Saving…' : 'Save storage warnings'}</button>
  {#if message}<p>{message}</p>{/if}
</section>
