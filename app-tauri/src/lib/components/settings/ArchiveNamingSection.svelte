<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { profilesState } from '$lib/stores/profiles.svelte';
  import FieldHelp from '$lib/components/FieldHelp.svelte';

  let archiveTemplate = $state('');
  let archivePreview = $state('');
  let archiveTemplateResult = $state('');
  let archiveTemplateSaving = $state(false);

  async function refreshArchivePreview() {
    try {
      archivePreview = await invoke<string>('preview_archive_name', { template: archiveTemplate });
    } catch (e) {
      archivePreview = `(${e})`;
    }
  }

  async function saveArchiveTemplate() {
    const id = profilesState.activeId;
    if (!id) {
      archiveTemplateResult = 'No active profile';
      return;
    }
    archiveTemplateSaving = true;
    try {
      const template = archiveTemplate.trim();
      await invoke('set_profile_template', { id, template: template || null });
      await profilesState.load();
      archiveTemplateResult = template ? 'Template saved' : 'Template reset to default';
    } catch (e) {
      archiveTemplateResult = `Failed: ${e}`;
    } finally {
      archiveTemplateSaving = false;
    }
  }

  onMount(() => {
    archiveTemplate = profilesState.active?.archive_template ?? '';
    refreshArchivePreview();
  });

  let lastActiveId = $state<string | null>(profilesState.activeId);
  $effect(() => {
    const id = profilesState.activeId;
    if (id === lastActiveId) return;
    lastActiveId = id;

    archiveTemplate = profilesState.active?.archive_template ?? '';
    refreshArchivePreview();
  });

  $effect(() => {
    void archiveTemplate;
    refreshArchivePreview();
  });
</script>

<form class="settings-form" onsubmit={(e) => { e.preventDefault(); saveArchiveTemplate(); }}>
  <fieldset class="form-group">
    <legend>Archive Naming</legend>
    <FieldHelp text="Each backup is saved under a name built from this template. Leave it blank to use the sensible default. Variables you can use:" />
    <ul class="var-help">
      <li><code>{'{date}'}</code> <span>today's date, e.g. 2026-05-31</span></li>
      <li><code>{'{time}'}</code> <span>the time, e.g. 143015</span></li>
      <li><code>{'{datetime}'}</code> <span>date and time together</span></li>
      <li><code>{'{hostname}'}</code> <span>this PC's name, e.g. her-pc</span></li>
      <li><code>{'{profile}'}</code> <span>the active profile name</span></li>
      <li><code>{'{random}'}</code> <span>a few random characters, keeps names unique</span></li>
    </ul>
    <FieldHelp
      examples={[
        { input: '{datetime}-{random}', output: '2026-05-31T143015-a1b2' },
        { input: '{hostname}-{date}', output: 'her-pc-2026-05-31' },
      ]}
    />

    <div class="field">
      <label for="archive-template">Template</label>
      <input id="archive-template" type="text" bind:value={archiveTemplate} placeholder="{'{datetime}-{random}'}" />
    </div>

    <div class="field">
      <span class="field-label">Preview</span>
      <code class="preview-box">{archivePreview || '...'}</code>
    </div>

    <div class="form-actions">
      <button type="submit" class="btn btn-primary" disabled={archiveTemplateSaving}>
        {archiveTemplateSaving ? 'Saving...' : 'Save'}
      </button>
    </div>

    {#if archiveTemplateResult}
      <div class="test-result" class:success={archiveTemplateResult.includes('saved') || archiveTemplateResult.includes('reset')} class:error={archiveTemplateResult.includes('Failed') || archiveTemplateResult.includes('No active')}>
        {archiveTemplateResult}
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

  .preview-box {
    display: block;
    background: var(--color-bg);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-sm);
    padding: var(--space-2) var(--space-3);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--color-accent);
    word-break: break-all;
  }

  .var-help {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin-top: var(--space-3);
    padding: var(--space-2) var(--space-3);
    background: var(--color-bg);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-sm);
  }

  .var-help li {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-size: var(--text-xs);
  }

  .var-help code {
    font-family: var(--font-mono);
    font-size: 0.7rem;
    color: var(--color-accent);
    flex-shrink: 0;
    min-width: 7.5rem;
  }

  .var-help span {
    color: var(--color-text-dim);
    line-height: 1.4;
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
