<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open, save as saveDialog } from '@tauri-apps/plugin-dialog';
  import { repoState, type RepoConfig } from '$lib/stores/repo.svelte';
  import { profilesState } from '$lib/stores/profiles.svelte';
  import FieldHelp from '$lib/components/FieldHelp.svelte';

  interface Props {
    repoFromForm: () => RepoConfig | null;
    repoType: () => 'ssh' | 'local';
  }

  let { repoFromForm, repoType }: Props = $props();

  let newProfileName = $state('');
  let profileResult = $state('');

  let renameModalOpen = $state(false);
  let renameInput = $state('');
  let deleteModalOpen = $state(false);

  async function addProfile() {
    const name = newProfileName.trim();
    if (!name) {
      profileResult = 'Name required';
      return;
    }
    const repo = repoFromForm();
    if (!repo) {
      profileResult = repoType() === 'local'
        ? 'Choose a backup folder first'
        : 'Fill in SSH host, user, and repo path first';
      return;
    }
    try {
      const created = await profilesState.create(name, repo);
      await profilesState.setActive(created.id);
      await repoState.load();
      newProfileName = '';
      profileResult = `Created profile "${name}"`;
    } catch (e) {
      profileResult = `Failed: ${e}`;
    }
  }

  function openRenameModal() {
    if (!profilesState.activeId) return;
    renameInput = profilesState.active?.name ?? '';
    renameModalOpen = true;
  }

  async function confirmRename() {
    const id = profilesState.activeId;
    const newName = renameInput.trim();
    if (!id || !newName) {
      renameModalOpen = false;
      return;
    }
    try {
      await profilesState.rename(id, newName);
      profileResult = `Renamed to "${newName}"`;
    } catch (e) {
      profileResult = `Failed: ${e}`;
    } finally {
      renameModalOpen = false;
    }
  }

  async function exportActive() {
    const id = profilesState.activeId;
    if (!id) return;
    const profile = profilesState.active;
    const suggestedName = `${profile?.id ?? 'profile'}.borgui.json`;
    try {
      const path = await saveDialog({
        title: 'Export profile',
        defaultPath: suggestedName,
        filters: [{ name: 'BorgUI profile', extensions: ['json'] }],
      });
      if (!path) return;
      await invoke('export_profile', { id, path });
      profileResult = `Exported to ${path}`;
    } catch (e) {
      profileResult = `Export failed: ${e}`;
    }
  }

  async function importProfile() {
    try {
      const path = await open({
        title: 'Import profile',
        multiple: false,
        filters: [{ name: 'BorgUI profile', extensions: ['json'] }],
      });
      if (!path) return;
      const imported = await invoke<{ id: string; name: string }>('import_profile', { path });
      await profilesState.load();
      await profilesState.setActive(imported.id);
      await repoState.load();
      profileResult = `Imported profile "${imported.name}"`;
    } catch (e) {
      profileResult = `Import failed: ${e}`;
    }
  }

  function openDeleteModal() {
    if (!profilesState.activeId) return;
    if (profilesState.profiles.length <= 1) {
      profileResult = 'Cannot delete the only profile';
      return;
    }
    deleteModalOpen = true;
  }

  async function confirmDeleteProfile() {
    const id = profilesState.activeId;
    if (!id) {
      deleteModalOpen = false;
      return;
    }
    try {
      await profilesState.remove(id);
      await repoState.load();
      profileResult = 'Profile deleted';
    } catch (e) {
      profileResult = `Failed: ${e}`;
    } finally {
      deleteModalOpen = false;
    }
  }

  // Close the rename/delete modals with the Escape key. Mirrors the
  // click-backdrop-to-close behaviour so modals are dismissable from the
  // keyboard too.
  $effect(() => {
    const anyOpen = renameModalOpen || deleteModalOpen;
    if (!anyOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      renameModalOpen = false;
      deleteModalOpen = false;
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });
</script>

<form class="settings-form" onsubmit={(e) => { e.preventDefault(); addProfile(); }}>
  <fieldset class="form-group">
    <legend>Profiles</legend>
    <FieldHelp
      text="A profile bundles one backup destination together with its schedule and retention rules. You only need more than one if you back up different things to different places. Switch between them with the picker in the sidebar."
      examples={[
        { input: 'Documents → office server' },
        { input: 'Photos → USB drive' },
      ]}
    />

    {#if profilesState.active}
      <div class="profile-current">
        <span class="field-label">Active</span>
        <code>{profilesState.active.name}</code>
        <div class="profile-actions">
          <button type="button" class="btn btn-secondary" onclick={openRenameModal}>Rename</button>
          <button type="button" class="btn btn-secondary" onclick={exportActive}>Export</button>
          <button type="button" class="btn btn-secondary" onclick={openDeleteModal} disabled={profilesState.profiles.length <= 1}>Delete</button>
        </div>
      </div>
    {/if}

    <div class="field">
      <label for="new-profile-name">New profile name</label>
      <div class="inline-row">
        <input id="new-profile-name" type="text" bind:value={newProfileName} placeholder="e.g. Work laptop" />
        <button type="submit" class="btn btn-primary">+ Add</button>
        <button type="button" class="btn btn-secondary" onclick={importProfile}>Import…</button>
      </div>
    </div>

    {#if profileResult}
      <div class="test-result" class:success={profileResult.includes('Created') || profileResult.includes('Renamed') || profileResult.includes('deleted') || profileResult.includes('Exported') || profileResult.includes('Imported')} class:error={profileResult.includes('failed') || profileResult.includes('Failed') || profileResult.includes('required') || profileResult.includes('Cannot')}>
        {profileResult}
      </div>
    {/if}
  </fieldset>
</form>

{#if renameModalOpen}
  <div class="modal-backdrop" onclick={() => (renameModalOpen = false)} role="presentation">
    <div
      class="modal"
      onclick={(e) => e.stopPropagation()}
      onkeydown={() => {}}
      role="dialog"
      tabindex="-1"
      aria-modal="true"
      aria-labelledby="rename-title"
    >
      <h2 id="rename-title">Rename profile</h2>
      <form onsubmit={(e) => { e.preventDefault(); confirmRename(); }}>
        <div class="field">
          <label for="rename-input">Profile name</label>
          <!-- svelte-ignore a11y_autofocus -->
          <input id="rename-input" type="text" bind:value={renameInput} autofocus />
        </div>
        <div class="modal-actions">
          <button type="button" class="btn btn-secondary" onclick={() => (renameModalOpen = false)}>Cancel</button>
          <button type="submit" class="btn btn-primary" disabled={!renameInput.trim()}>Save</button>
        </div>
      </form>
    </div>
  </div>
{/if}

{#if deleteModalOpen}
  <div class="modal-backdrop" onclick={() => (deleteModalOpen = false)} role="presentation">
    <div
      class="modal"
      onclick={(e) => e.stopPropagation()}
      onkeydown={() => {}}
      role="dialog"
      tabindex="-1"
      aria-modal="true"
      aria-labelledby="delete-profile-title"
    >
      <h2 id="delete-profile-title">Delete profile?</h2>
      <p>This removes the profile <code>{profilesState.active?.name}</code> and its settings (connection, schedule, retention). Your backups in the repository are NOT deleted.</p>
      <div class="modal-actions">
        <button type="button" class="btn btn-secondary" onclick={() => (deleteModalOpen = false)}>Cancel</button>
        <button type="button" class="btn btn-delete-confirm" onclick={confirmDeleteProfile}>Delete</button>
      </div>
    </div>
  </div>
{/if}

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

  .profile-current {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
    margin-top: var(--space-3);
    padding: var(--space-3);
    background: var(--color-bg);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
  }

  .profile-current code {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--color-text);
    flex: 1;
  }

  .profile-actions {
    display: flex;
    gap: var(--space-2);
  }

  .inline-row {
    display: flex;
    gap: var(--space-2);
  }

  .inline-row input {
    flex: 1;
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

  .btn-secondary {
    background: var(--color-surface-hover);
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
  }

  .btn-secondary:hover:not(:disabled) {
    background: var(--color-surface-active);
    color: var(--color-text);
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

  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: var(--color-backdrop);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .modal {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-6);
    max-width: 440px;
    width: 90%;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .modal h2 {
    font-size: var(--text-lg);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .modal p {
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    line-height: 1.5;
  }

  .modal code {
    font-family: var(--font-mono);
    color: var(--color-text);
    background: var(--color-surface-hover);
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
  }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-4);
  }
</style>
