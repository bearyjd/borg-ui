<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open, save } from '@tauri-apps/plugin-dialog';
  import FieldHelp from '$lib/components/FieldHelp.svelte';

  let exportPassphrase = $state('');
  let exportConfirm = $state('');
  let importPassphrase = $state('');
  let importPath = $state('');
  let importConfirmed = $state(false);
  let busy = $state(false);
  let result = $state('');

  async function exportKey() {
    if (!exportPassphrase) {
      result = 'Choose a recovery passphrase.';
      return;
    }
    if (exportPassphrase !== exportConfirm) {
      result = 'Recovery passphrases do not match.';
      return;
    }
    const path = await save({
      title: 'Export encrypted recovery key',
      defaultPath: 'borgui-recovery-key.json',
      filters: [{ name: 'BorgUI recovery key', extensions: ['json'] }],
    });
    if (!path) return;
    busy = true;
    try {
      await invoke('export_recovery_key', {
        path,
        recoveryPassphrase: exportPassphrase,
      });
      exportPassphrase = '';
      exportConfirm = '';
      result = 'Encrypted recovery key exported. Store the file and passphrase separately.';
    } catch (error) {
      result = `Recovery-key export failed: ${error}`;
    } finally {
      busy = false;
    }
  }

  async function chooseImport() {
    const selected = await open({
      title: 'Choose encrypted recovery key',
      multiple: false,
      directory: false,
      filters: [{ name: 'BorgUI recovery key', extensions: ['json'] }],
    });
    if (selected) importPath = selected as string;
  }

  async function importKey() {
    if (!importPath || !importPassphrase || !importConfirmed) return;
    busy = true;
    try {
      await invoke('import_recovery_key', {
        path: importPath,
        recoveryPassphrase: importPassphrase,
      });
      importPassphrase = '';
      importConfirmed = false;
      result = 'Repository key imported and validated by Borg.';
    } catch (error) {
      result = `Recovery-key import failed: ${error}`;
    } finally {
      busy = false;
    }
  }
</script>

<fieldset class="form-group">
  <legend>Encrypted recovery key</legend>
  <FieldHelp text="A recovery key can restore access if Borg's local key is lost. The export is encrypted with a separate passphrase using age/scrypt. BorgUI never includes it in profiles, configuration exports, logs, history, or support bundles." />
  <div class="warning">
    Anyone with this file and its passphrase may gain access to your encrypted repository.
    Store them separately. Losing both your Borg passphrase and this recovery material is irreversible.
  </div>

  <h3>Export</h3>
  <div class="fields">
    <input type="password" autocomplete="new-password" placeholder="Recovery passphrase" bind:value={exportPassphrase} />
    <input type="password" autocomplete="new-password" placeholder="Confirm recovery passphrase" bind:value={exportConfirm} />
    <button class="btn btn-secondary" type="button" disabled={busy} onclick={exportKey}>Export encrypted key…</button>
  </div>

  <h3>Recover</h3>
  <div class="fields">
    <button class="btn btn-secondary" type="button" disabled={busy} onclick={chooseImport}>
      {importPath ? 'Choose another file…' : 'Choose recovery file…'}
    </button>
    {#if importPath}<code>{importPath}</code>{/if}
    <input type="password" autocomplete="current-password" placeholder="Recovery passphrase" bind:value={importPassphrase} />
    <label class="confirm">
      <input type="checkbox" bind:checked={importConfirmed} />
      I understand this imports repository key material into this Borg installation.
    </label>
    <button class="btn btn-primary" type="button" disabled={busy || !importPath || !importPassphrase || !importConfirmed} onclick={importKey}>
      Import and validate key
    </button>
  </div>

  {#if result}<p>{result}</p>{/if}
</fieldset>

<style>
  .warning { border: 1px solid var(--color-warning); border-radius: var(--radius-md); padding: var(--space-3); color: var(--color-text-muted); font-size: var(--text-sm); }
  h3 { margin-top: var(--space-4); font-size: var(--text-base); }
  .fields { display: flex; flex-direction: column; align-items: flex-start; gap: var(--space-2); margin-top: var(--space-2); }
  input[type='password'] { width: min(100%, 420px); }
  .confirm { display: flex; gap: var(--space-2); align-items: flex-start; font-size: var(--text-sm); color: var(--color-text-muted); }
  code, p { font-size: var(--text-sm); overflow-wrap: anywhere; color: var(--color-text-muted); }
</style>
