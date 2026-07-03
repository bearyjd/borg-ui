<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { profilesState, type ResourcePolicy } from '$lib/stores/profiles.svelte';
  import FieldHelp from '$lib/components/FieldHelp.svelte';

  let policy = $state<ResourcePolicy>({
    skip_on_battery: false,
    prevent_sleep: true,
    wake_for_backup: false,
    upload_limit_kib: null,
    allowed_wifi_names: [],
    removable_destination_trigger: false,
  });
  let wifiNames = $state('');
  let uploadLimit = $state<number | null>(null);
  let consent = $state(false);
  let result = $state('');
  let saving = $state(false);

  onMount(async () => {
    try {
      policy = await invoke<ResourcePolicy>('load_resource_policy');
      wifiNames = policy.allowed_wifi_names.join(', ');
      uploadLimit = policy.upload_limit_kib;
    } catch (error) {
      result = `Could not load resource policy: ${error}`;
    }
  });

  async function save() {
    saving = true;
    result = '';
    try {
      policy.allowed_wifi_names = wifiNames
        .split(',')
        .map((name) => name.trim())
        .filter(Boolean);
      policy.upload_limit_kib = uploadLimit && uploadLimit > 0 ? uploadLimit : null;
      await invoke('save_resource_policy', {
        policy,
        autostartConsent: consent,
      });
      await profilesState.load();
      result = 'Resource policy saved.';
    } catch (error) {
      result = `Could not save resource policy: ${error}`;
    } finally {
      saving = false;
    }
  }

  async function snooze(choice: string) {
    try {
      await invoke('set_global_snooze', { choice });
      result = choice === 'clear' ? 'Automatic backups resumed.' : 'Automatic backups snoozed.';
    } catch (error) {
      result = `Could not update snooze: ${error}`;
    }
  }
</script>

<fieldset class="form-group">
  <legend>Resource-aware backups</legend>
  <FieldHelp text="Battery and Wi-Fi rules apply only to automatic backups. Manual backups always run, while still respecting bandwidth limits and preventing sleep." />

  <label><input type="checkbox" bind:checked={policy.skip_on_battery} /> Skip automatic backups on battery</label>
  <label><input type="checkbox" bind:checked={policy.prevent_sleep} /> Prevent automatic sleep during a backup</label>
  <label><input type="checkbox" bind:checked={policy.wake_for_backup} /> Wake this PC for scheduled backups</label>

  <label class="field">
    <span>SSH upload limit (KiB/s, blank is unlimited)</span>
    <input type="number" min="1" bind:value={uploadLimit} placeholder="Unlimited" />
  </label>
  <label class="field">
    <span>Allowed Wi-Fi names (comma-separated, blank allows all)</span>
    <input bind:value={wifiNames} placeholder="Home, Office" />
  </label>

  <label>
    <input type="checkbox" bind:checked={policy.removable_destination_trigger} />
    Back up when a removable local destination appears
  </label>
  {#if policy.removable_destination_trigger}
    <label class="consent">
      <input type="checkbox" bind:checked={consent} />
      I consent to BorgUI starting at login so it can detect the destination.
    </label>
  {/if}

  <div class="actions">
    <button class="btn btn-primary" type="button" onclick={save} disabled={saving}>
      {saving ? 'Saving…' : 'Save resource policy'}
    </button>
  </div>

  <div class="snooze">
    <strong>Snooze automatic backups</strong>
    <div class="actions">
      <button class="btn btn-secondary" onclick={() => snooze('one_hour')}>1 hour</button>
      <button class="btn btn-secondary" onclick={() => snooze('four_hours')}>4 hours</button>
      <button class="btn btn-secondary" onclick={() => snooze('tomorrow')}>Until tomorrow</button>
      <button class="btn btn-secondary" onclick={() => snooze('indefinite')}>Indefinitely</button>
      <button class="btn btn-secondary" onclick={() => snooze('clear')}>Resume</button>
    </div>
  </div>
  {#if result}<p>{result}</p>{/if}
</fieldset>

<style>
  .form-group { display: grid; gap: var(--space-3); }
  label { display: flex; align-items: center; gap: var(--space-2); color: var(--color-text-muted); }
  .field { align-items: stretch; flex-direction: column; }
  .field input { padding: var(--space-2) var(--space-3); border: 1px solid var(--color-border); border-radius: var(--radius-md); background: var(--color-bg); color: var(--color-text); }
  .consent { margin-left: var(--space-5); color: var(--color-warning); }
  .actions { display: flex; flex-wrap: wrap; gap: var(--space-2); }
  .snooze { display: grid; gap: var(--space-2); padding-top: var(--space-3); border-top: 1px solid var(--color-border-subtle); }
  p { margin: 0; color: var(--color-text-muted); }
</style>
