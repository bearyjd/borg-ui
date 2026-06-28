<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open, save } from '@tauri-apps/plugin-dialog';
  import { profilesState } from '$lib/stores/profiles.svelte';
  import FieldHelp from '$lib/components/FieldHelp.svelte';

  interface ImportPreview {
    format_version: number;
    added: string[];
    replaced: string[];
    removed: string[];
    active_profile: string | null;
  }

  let busy = $state(false);
  let result = $state('');
  let importPath = $state('');
  let preview = $state<ImportPreview | null>(null);

  async function run(action: () => Promise<void>, success: string) {
    busy = true;
    result = '';
    try {
      await action();
      result = success;
    } catch (e) {
      result = `Failed: ${e}`;
    } finally {
      busy = false;
    }
  }

  async function exportSupportBundle() {
    const path = await save({
      title: 'Export support bundle',
      defaultPath: 'borgui-support.zip',
      filters: [{ name: 'ZIP archive', extensions: ['zip'] }],
    });
    if (path) {
      await run(
        () => invoke('export_support_bundle', { path }),
        'Support bundle exported.',
      );
    }
  }

  async function exportConfiguration() {
    const path = await save({
      title: 'Export redacted configuration',
      defaultPath: 'borgui-configuration.json',
      filters: [{ name: 'JSON document', extensions: ['json'] }],
    });
    if (path) {
      await run(
        () => invoke('export_configuration', { path }),
        'Redacted configuration exported.',
      );
    }
  }

  async function chooseImport() {
    const path = await open({
      title: 'Import BorgUI configuration',
      multiple: false,
      directory: false,
      filters: [{ name: 'JSON document', extensions: ['json'] }],
    });
    if (!path) return;
    busy = true;
    result = '';
    preview = null;
    try {
      importPath = path as string;
      preview = await invoke<ImportPreview>('preview_configuration_import', {
        path: importPath,
      });
    } catch (e) {
      result = `Failed: ${e}`;
      importPath = '';
    } finally {
      busy = false;
    }
  }

  async function confirmImport() {
    await run(async () => {
      await invoke('import_configuration', { path: importPath });
      await profilesState.load();
      preview = null;
      importPath = '';
    }, 'Configuration imported. A rollback copy was saved.');
  }
</script>

<form class="settings-form" onsubmit={(event) => event.preventDefault()}>
  <fieldset class="form-group">
    <legend>Diagnostics</legend>
    <FieldHelp text="Diagnostics stay on this computer unless you explicitly export them. Exports never include saved passphrases, SSH private keys, archive contents, or source file listings." />

    <div class="actions">
      <button class="btn btn-secondary" type="button" disabled={busy} onclick={() => run(
        () => invoke('open_log_folder'),
        'Log folder opened.',
      )}>Open log folder</button>
      <button class="btn btn-secondary" type="button" disabled={busy} onclick={exportSupportBundle}>
        Export support bundle
      </button>
      <button class="btn btn-secondary" type="button" disabled={busy} onclick={exportConfiguration}>
        Export configuration
      </button>
      <button class="btn btn-secondary" type="button" disabled={busy} onclick={chooseImport}>
        Import configuration
      </button>
    </div>

    {#if preview}
      <div class="preview" role="status">
        <strong>Import preview</strong>
        <p>Format version {preview.format_version}</p>
        <p>Will add: {preview.added.length ? preview.added.join(', ') : 'none'}</p>
        <p>Will replace: {preview.replaced.length ? preview.replaced.join(', ') : 'none'}</p>
        <p class:destructive={preview.removed.length > 0}>
          Will remove: {preview.removed.length ? preview.removed.join(', ') : 'none'}
        </p>
        <div class="confirm-actions">
          <button class="btn btn-secondary" type="button" onclick={() => {
            preview = null;
            importPath = '';
          }}>Cancel</button>
          <button class="btn btn-primary" type="button" disabled={busy} onclick={confirmImport}>
            Confirm import
          </button>
        </div>
      </div>
    {/if}

    {#if result}
      <div class="result" class:error={result.startsWith('Failed')} role="status">{result}</div>
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
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-top: var(--space-4);
  }
  .preview, .result {
    margin-top: var(--space-4);
    padding: var(--space-3);
    border-radius: var(--radius-sm);
    background: var(--color-surface-hover);
    color: var(--color-text-muted);
    font-size: var(--text-sm);
  }
  .preview strong {
    color: var(--color-text);
  }
  .preview .destructive {
    color: var(--color-danger);
    font-weight: 600;
  }
  .confirm-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-3);
  }
  .result.error {
    color: var(--color-danger);
    background: var(--color-danger-muted);
  }
</style>
