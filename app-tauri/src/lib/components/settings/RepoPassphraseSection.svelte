<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { repoForm } from '$lib/stores/repo-form.svelte';
  import { profilesState } from '$lib/stores/profiles.svelte';
  import { explainConnectionError, type HintContext } from '$lib/connection-hints';
  import { passphraseFailureMessage, planPassphraseSave } from '$lib/passphrase-save';
  import FieldHelp from '$lib/components/FieldHelp.svelte';

  let hasPassphrase = $state(false);
  let passphraseLoading = $state(false);
  let passphraseModalOpen = $state(false);
  let passphraseInput = $state('');
  let passphraseConfirm = $state('');
  let passphraseSaving = $state(false);
  let passphraseResult = $state('');
  // Set explicitly alongside every `passphraseResult`. The banner used to sniff
  // the message text for 'saved'/'match'/'Failed', which silently mis-styled any
  // new wording — a success ending in "…updated to match." rendered as an error.
  let passphraseStatus = $state<'success' | 'error' | ''>('');
  let passphraseHint = $state('');
  let clearPassphraseModalOpen = $state(false);
  // Change-flow escape hatch: overwrite only the stored copy without rotating
  // the repository's real passphrase (for repairing an out-of-sync keychain).
  let storeOnly = $state(false);

  function hintFor(e: unknown): string {
    const contexts: HintContext[] = repoForm.repoType === 'ssh' ? ['ssh', 'repo'] : ['repo'];
    return explainConnectionError(String(e), contexts) ?? '';
  }

  /** Re-check keychain status; also called by the page after a repo init. */
  export async function refresh() {
    const repo = repoForm.currentRepoFromForm();
    if (!repo) {
      hasPassphrase = false;
      return;
    }
    passphraseLoading = true;
    try {
      hasPassphrase = await invoke<boolean>('has_repo_passphrase', { repo });
    } catch {
      hasPassphrase = false;
    } finally {
      passphraseLoading = false;
    }
  }

  async function openPassphraseModal() {
    passphraseInput = '';
    passphraseConfirm = '';
    passphraseResult = '';
    passphraseStatus = '';
    passphraseHint = '';
    storeOnly = false;
    // `hasPassphrase` decides rotate-vs-store, and the command runs against
    // whatever the repo form currently holds. `refresh()` otherwise only fires
    // on mount and on profile switch, so editing the connection fields to point
    // at a different repository would leave this stale — and a stale `false`
    // silently downgrades a real change into a keychain-only write, which is
    // exactly the desync this component is supposed to prevent. Re-check
    // against the live form before opening.
    await refresh();
    passphraseModalOpen = true;
  }

  async function savePassphrase() {
    const repo = repoForm.currentRepoFromForm();
    if (!repo) {
      passphraseResult = 'Configure SSH connection first.';
      passphraseStatus = 'error';
      return;
    }
    const plan = planPassphraseSave({
      hasStoredPassphrase: hasPassphrase,
      storeOnly,
      passphrase: passphraseInput,
      confirm: passphraseConfirm
    });
    if (plan.kind === 'invalid') {
      passphraseResult = plan.message;
      passphraseStatus = 'error';
      return;
    }
    passphraseSaving = true;
    try {
      // `plan.command` encodes the rotate-vs-store decision (see
      // $lib/passphrase-save): changing an existing passphrase must rotate the
      // REPOSITORY's own passphrase, not just overwrite the stored copy.
      if (plan.mode === 'rotate') {
        await invoke(plan.command, { repo, newPassphrase: passphraseInput });
      } else {
        await invoke(plan.command, { repo, passphrase: passphraseInput });
      }
      hasPassphrase = true;
      passphraseModalOpen = false;
      passphraseInput = '';
      passphraseConfirm = '';
      passphraseResult = plan.successMessage;
      passphraseStatus = 'success';
    } catch (e) {
      passphraseResult = passphraseFailureMessage(plan.mode, e);
      passphraseStatus = 'error';
      passphraseHint = hintFor(e);
    } finally {
      passphraseSaving = false;
    }
  }

  async function confirmClearPassphrase() {
    const repo = repoForm.currentRepoFromForm();
    if (!repo) {
      clearPassphraseModalOpen = false;
      return;
    }
    try {
      await invoke('clear_repo_passphrase', { repo });
      hasPassphrase = false;
      passphraseResult = 'Passphrase removed from keychain.';
      passphraseStatus = 'success';
    } catch (e) {
      passphraseResult = `Failed to clear passphrase: ${e}`;
      passphraseStatus = 'error';
    } finally {
      clearPassphraseModalOpen = false;
    }
  }

  // Close the passphrase modals with the Escape key. Mirrors the
  // click-backdrop-to-close behaviour so modals are dismissable from the
  // keyboard too.
  $effect(() => {
    const anyOpen = clearPassphraseModalOpen || passphraseModalOpen;
    if (!anyOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      clearPassphraseModalOpen = false;
      passphraseModalOpen = false;
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  onMount(async () => {
    await refresh();
  });

  let lastActiveId = $state<string | null>(profilesState.activeId);
  $effect(() => {
    const id = profilesState.activeId;
    if (id === lastActiveId) return;
    lastActiveId = id;

    refresh();
  });
</script>

<form class="settings-form" onsubmit={(e) => e.preventDefault()}>
  <fieldset class="form-group">
    <legend>Repository Passphrase</legend>
    <FieldHelp text="This is the passphrase that encrypts the repository itself — the one you chose when you initialized it. It is NOT the SSH key password. It's saved securely in Windows Credential Manager and used automatically for every backup and restore, so you won't be asked for it each time." />

    <div class="passphrase-status">
      <span class="status-dot" class:set={hasPassphrase}></span>
      <span>
        {#if passphraseLoading}
          Checking…
        {:else if hasPassphrase}
          Passphrase is set for this repository
        {:else}
          No passphrase stored
        {/if}
      </span>
    </div>

    <div class="form-actions">
      <button type="button" class="btn btn-primary" onclick={openPassphraseModal} disabled={!repoForm.configured}>
        {hasPassphrase ? 'Change passphrase' : 'Set passphrase'}
      </button>
      {#if hasPassphrase}
        <button type="button" class="btn btn-secondary" onclick={() => (clearPassphraseModalOpen = true)}>
          Clear
        </button>
      {/if}
    </div>

    {#if passphraseResult && !passphraseModalOpen}
      <div class="test-result" class:success={passphraseStatus === 'success'} class:error={passphraseStatus === 'error'}>
        {passphraseResult}
      </div>
    {/if}
  </fieldset>
</form>

{#if clearPassphraseModalOpen}
  <div class="modal-backdrop" onclick={() => (clearPassphraseModalOpen = false)} role="presentation">
    <div
      class="modal"
      onclick={(e) => e.stopPropagation()}
      onkeydown={() => {}}
      role="dialog"
      tabindex="-1"
      aria-modal="true"
      aria-labelledby="clear-pass-title"
    >
      <h2 id="clear-pass-title">Remove passphrase?</h2>
      <p>This removes the saved passphrase from Windows Credential Manager. Backups and restores will fail until you set it again. Your existing backups are not affected.</p>
      <div class="modal-actions">
        <button type="button" class="btn btn-secondary" onclick={() => (clearPassphraseModalOpen = false)}>Cancel</button>
        <button type="button" class="btn btn-delete-confirm" onclick={confirmClearPassphrase}>Remove</button>
      </div>
    </div>
  </div>
{/if}

{#if passphraseModalOpen}
  <div class="modal-backdrop" onclick={() => (passphraseModalOpen = false)} role="presentation">
    <div
      class="modal"
      onclick={(e) => e.stopPropagation()}
      onkeydown={() => {}}
      role="dialog"
      tabindex="-1"
      aria-modal="true"
      aria-labelledby="passphrase-title"
    >
      <h2 id="passphrase-title">{hasPassphrase ? 'Change passphrase' : 'Set passphrase'}</h2>
      {#if hasPassphrase}
        <p>
          This re-encrypts this repository's key with a new passphrase, using the currently stored
          one, then updates the stored copy to match.
        </p>
        <p class="modal-note">
          For <code>keyfile</code> repositories the key is held on this PC, so the change applies
          here only — other machines using their own copy of the key keep the old passphrase. Any
          recovery key you exported earlier still carries the <em>old</em> passphrase; export a
          fresh one afterwards.
        </p>
      {:else}
        <p>Enter the passphrase used to encrypt this borg repository. It will be stored in your OS keychain.</p>
      {/if}
      <form onsubmit={(e) => { e.preventDefault(); savePassphrase(); }}>
        <div class="field">
          <label for="pass-input">Passphrase</label>
          <input id="pass-input" type="password" autocomplete="new-password" bind:value={passphraseInput} />
        </div>
        <div class="field">
          <label for="pass-confirm">Confirm</label>
          <input id="pass-confirm" type="password" autocomplete="new-password" bind:value={passphraseConfirm} />
        </div>
        {#if hasPassphrase}
          <label class="store-only">
            <input type="checkbox" bind:checked={storeOnly} />
            <span>Only update the stored copy — use this if the saved passphrase is wrong and backups fail to unlock. The repository's own passphrase is not changed.</span>
          </label>
        {/if}
        {#if passphraseResult}
          <div class="test-result error">
            {passphraseResult}
            {#if passphraseHint}<span class="result-hint">{passphraseHint}</span>{/if}
          </div>
        {/if}
        <div class="modal-actions">
          <button type="button" class="btn btn-secondary" onclick={() => (passphraseModalOpen = false)}>Cancel</button>
          <button type="submit" class="btn btn-primary" disabled={passphraseSaving}>
            {passphraseSaving ? 'Saving…' : 'Save'}
          </button>
        </div>
      </form>
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

  .passphrase-status {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin-top: var(--space-3);
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--color-text-dim);
  }

  .status-dot.set {
    background: var(--color-success);
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

  .result-hint {
    display: block;
    margin-top: var(--space-2);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
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

  /* Caveats that survive a skim — rotation is irreversible, so the keyfile and
     stale-recovery-key notes must not read as body copy. */
  .modal-note {
    margin-top: var(--space-2);
    padding-left: var(--space-3);
    border-left: 2px solid var(--color-border);
  }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-4);
  }
</style>
