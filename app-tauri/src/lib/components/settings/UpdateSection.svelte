<script lang="ts">
  import { updateState } from '$lib/stores/update.svelte';
  import FieldHelp from '$lib/components/FieldHelp.svelte';
</script>

<fieldset class="form-group">
  <legend>Application updates</legend>
  <FieldHelp text="BorgUI checks for signed releases when it starts and when you ask here. It never downloads or installs an update until you confirm." />
  <button class="btn btn-secondary" type="button" disabled={updateState.status === 'checking' || updateState.status === 'installing'} onclick={() => updateState.check()}>
    {updateState.status === 'checking' ? 'Checking…' : 'Check for updates'}
  </button>
  {#if updateState.status === 'current'}<p>BorgUI is up to date.</p>{/if}
  {#if updateState.status === 'available'}<p>Version {updateState.version} is available. Review and confirm in the update prompt.</p>{/if}
  {#if updateState.status === 'installing'}<p>Downloading update… {updateState.total ? `${Math.round(updateState.downloaded / updateState.total * 100)}%` : ''}</p>{/if}
  {#if updateState.status === 'error'}<p class="error">{updateState.error}</p>{/if}
</fieldset>

<style>
  p { margin-top: var(--space-2); color: var(--color-text-muted); font-size: var(--text-sm); }
  .error { color: var(--color-error); }
</style>
