<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import FieldHelp from '$lib/components/FieldHelp.svelte';

  let enabled = $state(false);
  let busy = $state(false);
  let result = $state('');

  onMount(async () => {
    try {
      enabled = await invoke<boolean>('get_autostart');
    } catch (e) {
      // Non-fatal: if we can't read the current state, leave the toggle off.
      console.warn('Failed to read autostart state:', e);
    }
  });

  async function toggle(value: boolean) {
    busy = true;
    result = '';
    try {
      await invoke('set_autostart', { enabled: value });
      enabled = value;
      result = value
        ? 'BorgUI will start in the tray when you sign in.'
        : 'BorgUI will no longer start automatically.';
    } catch (e) {
      // Re-sync the checkbox with the real registry state on failure.
      enabled = await invoke<boolean>('get_autostart').catch(() => false);
      result = `Failed: ${e}`;
    } finally {
      busy = false;
    }
  }
</script>

<form class="settings-form" onsubmit={(e) => e.preventDefault()}>
  <fieldset class="form-group">
    <legend>Startup</legend>
    <FieldHelp text="Start BorgUI automatically when you sign in to Windows. It opens minimized to the system tray (no window), ready in the background so you can run a backup or restore at any time from the tray icon." />

    <div class="field">
      <label class="toggle-row">
        <input
          type="checkbox"
          checked={enabled}
          disabled={busy}
          onchange={(e) => toggle(e.currentTarget.checked)}
        />
        <span>Start BorgUI when I sign in to Windows</span>
      </label>
    </div>

    {#if result}
      <div
        class="test-result"
        class:success={result.includes('will')}
        class:error={result.includes('Failed')}
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

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin-top: var(--space-4);
  }

  .toggle-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    cursor: pointer;
    font-size: var(--text-sm);
    color: var(--color-text);
  }

  .toggle-row input[type='checkbox'] {
    width: 16px;
    height: 16px;
    accent-color: var(--color-accent);
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
