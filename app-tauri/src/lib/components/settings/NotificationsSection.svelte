<script lang="ts">
  import { notificationsState } from '$lib/stores/notifications.svelte';
  import FieldHelp from '$lib/components/FieldHelp.svelte';

  // Local mirror of the store value so the checkbox reflects the user's
  // *attempt* even when the OS rejects permission, then we roll it back.
  let notificationsEnabled = $state(notificationsState.enabled);
  let notificationsResult = $state('');

  async function toggleNotifications(value: boolean) {
    notificationsResult = '';
    try {
      await notificationsState.setEnabled(value);
      notificationsEnabled = notificationsState.enabled;
      notificationsResult = value ? 'Notifications enabled.' : 'Notifications disabled.';
    } catch (e) {
      notificationsEnabled = false;
      notificationsResult = `${e}`;
    }
  }
</script>

<form class="settings-form" onsubmit={(e) => e.preventDefault()}>
  <fieldset class="form-group">
    <legend>Notifications</legend>
    <FieldHelp text="Show a desktop notification when a backup or restore finishes or fails. The first time you turn this on, Windows will ask for permission to show notifications — choose Allow. If you miss that prompt, you can enable notifications for BorgUI later in Windows Settings." />

    <div class="field">
      <label class="toggle-row">
        <input type="checkbox" checked={notificationsEnabled} onchange={(e) => toggleNotifications(e.currentTarget.checked)} />
        <span>Enable desktop notifications</span>
      </label>
    </div>

    {#if notificationsResult}
      <div class="test-result" class:success={notificationsResult.includes('enabled') || notificationsResult.includes('disabled')} class:error={notificationsResult.includes('denied') || notificationsResult.includes('Error')}>
        {notificationsResult}
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

  .field label {
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-text-muted);
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

  .toggle-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    cursor: pointer;
    font-size: var(--text-sm);
    color: var(--color-text);
  }

  .toggle-row input[type="checkbox"] {
    width: 16px;
    height: 16px;
    accent-color: var(--color-accent);
  }
</style>
