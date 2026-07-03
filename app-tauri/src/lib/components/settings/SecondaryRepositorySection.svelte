<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { profilesState } from '$lib/stores/profiles.svelte';
  import type { RepoConfig } from '$lib/stores/repo.svelte';
  import FieldHelp from '$lib/components/FieldHelp.svelte';

  let enabled = $state(!!profilesState.active?.secondary_repo);
  let type = $state<'ssh' | 'local'>(
    profilesState.active?.secondary_repo?.ssh_host ? 'ssh' : 'local',
  );
  let host = $state(profilesState.active?.secondary_repo?.ssh_host ?? '');
  let port = $state(profilesState.active?.secondary_repo?.ssh_port || 22);
  let user = $state(profilesState.active?.secondary_repo?.ssh_user ?? '');
  let path = $state(profilesState.active?.secondary_repo?.repo_path ?? '');
  let keyPath = $state(profilesState.active?.secondary_repo?.ssh_key_path ?? '');
  let passphrase = $state('');
  let result = $state('');

  async function save() {
    let repo: RepoConfig | null = null;
    if (enabled) {
      repo = type === 'local'
        ? { ssh_host: '', ssh_port: 0, ssh_user: '', repo_path: path, ssh_key_path: null }
        : {
            ssh_host: host.trim(),
            ssh_port: port,
            ssh_user: user.trim(),
            repo_path: path.trim(),
            ssh_key_path: keyPath.trim() || null,
          };
    }
    try {
      await invoke('save_secondary_repository', {
        repo,
        passphrase: passphrase || null,
      });
      passphrase = '';
      await profilesState.load();
      result = enabled ? 'Secondary destination saved.' : 'Secondary destination removed.';
    } catch (error) {
      result = `Could not save secondary destination: ${error}`;
    }
  }
</script>

<fieldset class="form-group">
  <legend>Secondary backup destination</legend>
  <FieldHelp text="Optional 3-2-1 protection. Each backup uses one source snapshot and the same archive name, writing primary first and secondary second. Retention is applied independently." />
  <label><input type="checkbox" bind:checked={enabled} /> Enable a secondary destination</label>
  {#if enabled}
    <label class="field"><span>Type</span><select bind:value={type}><option value="local">Local / USB / share</option><option value="ssh">SSH server</option></select></label>
    {#if type === 'ssh'}
      <div class="grid">
        <label class="field"><span>Host</span><input bind:value={host} /></label>
        <label class="field"><span>Port</span><input type="number" min="1" max="65535" bind:value={port} /></label>
        <label class="field"><span>User</span><input bind:value={user} /></label>
        <label class="field"><span>SSH key path</span><input bind:value={keyPath} /></label>
      </div>
    {/if}
    <label class="field"><span>Repository path</span><input bind:value={path} /></label>
    <label class="field"><span>Passphrase (stored separately in Credential Manager)</span><input type="password" bind:value={passphrase} placeholder="Leave blank to keep existing" /></label>
  {/if}
  <button class="btn btn-primary" onclick={save}>Save secondary destination</button>
  {#if result}<p>{result}</p>{/if}
</fieldset>

<style>
  .form-group { display: grid; gap: var(--space-3); }
  label { display: flex; align-items: center; gap: var(--space-2); color: var(--color-text-muted); }
  .field { align-items: stretch; flex-direction: column; }
  .grid { display: grid; grid-template-columns: repeat(2, 1fr); gap: var(--space-3); }
  input, select { padding: var(--space-2) var(--space-3); border: 1px solid var(--color-border); border-radius: var(--radius-md); background: var(--color-bg); color: var(--color-text); }
  p { color: var(--color-text-muted); }
</style>
