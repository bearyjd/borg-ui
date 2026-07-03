<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { profilesState, type HardeningPosture } from '$lib/stores/profiles.svelte';
  import { isLocalRepo } from '$lib/stores/repo.svelte';
  import FieldHelp from '$lib/components/FieldHelp.svelte';

  let posture = $state<HardeningPosture>({
    append_only_declared: false,
    restricted_ssh_declared: false,
    encrypted_repository_declared: false,
    recovery_key_exported: false,
    server_maintenance_documented: false,
  });
  let instructions = $state('');
  let notes = $state<string[]>([]);
  let checks = $state<Array<{ id: string; label: string; complete: boolean }>>([]);
  let result = $state('');
  let local = $derived(profilesState.active ? isLocalRepo(profilesState.active.repo) : false);

  onMount(async () => {
    posture = { ...posture, ...(profilesState.active?.hardening ?? {}) };
    await refreshChecks();
  });

  async function refreshChecks() {
    checks = await invoke('hardening_checklist');
  }

  async function generate() {
    try {
      const generated = await invoke<{ authorized_keys_line: string; maintenance_notes: string[] }>(
        'generate_append_only_instructions',
      );
      instructions = generated.authorized_keys_line;
      notes = generated.maintenance_notes;
      result = '';
    } catch (error) {
      result = `Could not generate instructions: ${error}`;
    }
  }

  async function save() {
    try {
      await invoke('save_hardening_posture', { posture });
      await profilesState.load();
      await refreshChecks();
      result = 'Repository hardening posture saved.';
    } catch (error) {
      result = `Could not save hardening posture: ${error}`;
    }
  }
</script>

<fieldset class="form-group">
  <legend>Ransomware resilience</legend>
  <FieldHelp text="Append-only SSH access prevents a compromised backup PC from permanently destroying repository data. It does not replace trusted server-side maintenance or Borg transaction-log recovery procedures." />

  {#if local}
    <p>Restricted SSH and append-only server access do not apply to this local repository. Keep removable copies disconnected when not in use.</p>
  {:else}
    <button class="btn btn-secondary" type="button" onclick={generate}>Generate authorized_keys instructions</button>
    {#if instructions}
      <label class="field">
        <span>Install this exact line in the backup server account’s <code>authorized_keys</code>:</span>
        <textarea readonly rows="5" value={instructions}></textarea>
      </label>
      <ul>{#each notes as note}<li>{note}</li>{/each}</ul>
    {/if}
    <label><input type="checkbox" bind:checked={posture.restricted_ssh_declared} /> Restricted backup-only SSH access is installed</label>
    <label><input type="checkbox" bind:checked={posture.append_only_declared} /> Backup access is declared append-only</label>
    <label><input type="checkbox" bind:checked={posture.server_maintenance_documented} /> Server maintenance and transaction-log recovery are documented</label>
    <p class="warning">Keep unrestricted maintenance credentials off this backup PC. BorgUI never asks for or stores them.</p>
  {/if}
  <label><input type="checkbox" bind:checked={posture.encrypted_repository_declared} /> Repository encryption is confirmed</label>

  {#if posture.append_only_declared}
    <p class="warning">Compact is disabled here. Prune and delete are logical operations; physical cleanup requires trusted server-side maintenance.</p>
  {/if}

  <button class="btn btn-primary" type="button" onclick={save}>Save hardening posture</button>

  <div class="checklist">
    <strong>Protection posture</strong>
    {#each checks as check}
      <span class:complete={check.complete}>{check.complete ? '✓' : '○'} {check.label}</span>
    {/each}
  </div>
  {#if result}<p>{result}</p>{/if}
</fieldset>

<style>
  .form-group, .checklist { display: grid; gap: var(--space-3); }
  label { display: flex; align-items: center; gap: var(--space-2); color: var(--color-text-muted); }
  .field { align-items: stretch; flex-direction: column; }
  textarea { width: 100%; padding: var(--space-3); border: 1px solid var(--color-border); border-radius: var(--radius-md); background: var(--color-bg); color: var(--color-text); font-family: var(--font-mono); }
  p, li { color: var(--color-text-muted); font-size: var(--text-sm); }
  .warning { color: var(--color-warning); }
  .complete { color: var(--color-success); }
  ul { margin: 0; padding-left: var(--space-5); }
</style>
