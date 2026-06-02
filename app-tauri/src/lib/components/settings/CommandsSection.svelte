<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { profilesState } from '$lib/stores/profiles.svelte';
  import FieldHelp from '$lib/components/FieldHelp.svelte';

  let preBackup = $state('');
  let postBackup = $state('');
  let result = $state('');
  let saving = $state(false);

  function loadFromProfile() {
    preBackup = profilesState.active?.pre_backup ?? '';
    postBackup = profilesState.active?.post_backup ?? '';
    result = '';
  }

  async function save() {
    const id = profilesState.activeId;
    if (!id) {
      result = 'No active profile';
      return;
    }
    saving = true;
    try {
      await invoke('set_profile_hooks', {
        id,
        preBackup: preBackup.trim() || null,
        postBackup: postBackup.trim() || null,
      });
      await profilesState.load();
      result = 'Commands saved';
    } catch (e) {
      result = `Failed: ${e}`;
    } finally {
      saving = false;
    }
  }

  onMount(loadFromProfile);

  let lastActiveId = $state<string | null>(profilesState.activeId);
  $effect(() => {
    const id = profilesState.activeId;
    if (id === lastActiveId) return;
    lastActiveId = id;
    loadFromProfile();
  });
</script>

<form class="settings-form" onsubmit={(e) => { e.preventDefault(); save(); }}>
  <fieldset class="form-group">
    <legend>Pre / Post Commands</legend>
    <FieldHelp text="Optionally run a command before and/or after each backup — for example, dump a database before backing it up, or notify a service when it finishes. Commands run in your system shell. Leave blank to skip." />
    <ul class="var-help">
      <li><code>$archive_name</code> <span>the name of the archive being created</span></li>
      <li><code>$repo_url</code> <span>the repository location (path or ssh:// URL)</span></li>
    </ul>
    <FieldHelp text="If the pre-backup command fails (exits non-zero), the backup is aborted — so a failed preparation step never produces a stale backup. A failing post-backup command is reported as a warning; the backup itself still succeeded." />

    <div class="field">
      <label for="pre-backup">Before backup</label>
      <textarea
        id="pre-backup"
        bind:value={preBackup}
        rows="2"
        placeholder="e.g. pg_dump mydb > C:\backup\mydb.sql"
        spellcheck="false"
      ></textarea>
    </div>

    <div class="field">
      <label for="post-backup">After backup</label>
      <textarea
        id="post-backup"
        bind:value={postBackup}
        rows="2"
        placeholder="e.g. curl -fsS https://hc-ping.com/your-uuid"
        spellcheck="false"
      ></textarea>
    </div>

    <div class="form-actions">
      <button type="submit" class="btn btn-primary" disabled={saving}>
        {saving ? 'Saving...' : 'Save'}
      </button>
    </div>

    {#if result}
      <div
        class="test-result"
        class:success={result.includes('saved')}
        class:error={result.includes('Failed') || result.includes('No active')}
      >
        {result}
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

  .field label {
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-text-muted);
  }

  .field textarea {
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: var(--space-2) var(--space-3);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    resize: vertical;
    transition: border-color var(--duration-fast) var(--ease-out);
  }

  .field textarea:focus {
    outline: none;
    border-color: var(--color-accent);
  }

  .field textarea::placeholder {
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
