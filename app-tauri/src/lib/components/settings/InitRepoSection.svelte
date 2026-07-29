<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { repoForm } from '$lib/stores/repo-form.svelte';
  import { explainConnectionError, type HintContext } from '$lib/connection-hints';
  import FieldHelp from '$lib/components/FieldHelp.svelte';

  interface Props {
    /** Called after a successful init so the passphrase section can refresh. */
    onInitialized: () => void;
  }

  let { onInitialized }: Props = $props();

  let initEncryption = $state<'repokey' | 'keyfile' | 'repokey-blake2' | 'keyfile-blake2' | 'authenticated' | 'authenticated-blake2' | 'none'>('repokey-blake2');
  let initPassphrase = $state('');
  let initPassphraseConfirm = $state('');
  let initing = $state(false);
  let initResult = $state('');
  let initHint = $state('');
  let needsPassphrase = $derived(
    initEncryption !== 'none' &&
    initEncryption !== 'authenticated' &&
    initEncryption !== 'authenticated-blake2'
  );

  function hintFor(e: unknown): string {
    const contexts: HintContext[] = repoForm.repoType === 'ssh' ? ['ssh', 'repo'] : ['repo'];
    return explainConnectionError(String(e), contexts) ?? '';
  }

  async function initRepo() {
    initResult = '';
    initHint = '';
    if (needsPassphrase) {
      if (!initPassphrase) {
        initResult = 'Passphrase required for this encryption mode.';
        return;
      }
      if (initPassphrase !== initPassphraseConfirm) {
        initResult = 'Passphrases do not match.';
        return;
      }
    }

    initing = true;
    try {
      await invoke('init_repo', {
        repo: repoForm.buildRepoConfig(),
        encryption: initEncryption,
        passphrase: needsPassphrase ? initPassphrase : null,
      });
      initResult = 'Repository initialized successfully.';
      initPassphrase = '';
      initPassphraseConfirm = '';
      onInitialized();
    } catch (e) {
      initResult = `Init failed: ${e}`;
      initHint = hintFor(e);
    } finally {
      initing = false;
    }
  }
</script>

<form class="settings-form" onsubmit={(e) => { e.preventDefault(); initRepo(); }}>
  <fieldset class="form-group">
    <legend>Initialize Repository</legend>
    <FieldHelp text="“Initialize” sets up a fresh, empty backup repository at the destination above. Do this once for a brand-new destination. Skip it if you're connecting to a backup that already exists — initializing again is not needed." />

    <details class="advanced-options">
      <summary>
        {initEncryption === 'repokey-blake2'
          ? 'Encryption: Recommended'
          : `Encryption: ${initEncryption}`}
      </summary>
      <div class="field">
        <label for="init-encryption">Encryption method</label>
        <select id="init-encryption" bind:value={initEncryption}>
          <option value="repokey-blake2">repokey-blake2 (recommended)</option>
          <option value="repokey">repokey</option>
          <option value="keyfile-blake2">keyfile-blake2</option>
          <option value="keyfile">keyfile</option>
          <option value="authenticated-blake2">authenticated-blake2 (no encryption)</option>
          <option value="authenticated">authenticated (no encryption)</option>
          <option value="none">none (no encryption, no auth)</option>
        </select>
        <ul class="var-help">
          <li><code>repokey-blake2</code> <span>recommended — encrypts your files; the key lives inside the repository</span></li>
          <li><code>repokey</code> <span>same idea, slightly slower checksum</span></li>
          <li><code>keyfile-blake2</code> <span>encrypts your files; key stored on this PC (back it up!)</span></li>
          <li><code>keyfile</code> <span>same, slightly slower checksum</span></li>
          <li><code>authenticated</code> <span>tamper-detection only</span></li>
          <li><code>none</code> <span>no protection at all</span></li>
        </ul>
      </div>
    </details>

    {#if initEncryption === 'none' || initEncryption.startsWith('authenticated')}
      <div class="warning-box">
        <strong>Not encrypted:</strong> anyone with access to this repository can read
        your files. Use the recommended encryption for private data.
      </div>
    {/if}

    {#if needsPassphrase}
      <div class="field">
        <label for="init-passphrase">Passphrase</label>
        <input id="init-passphrase" type="password" bind:value={initPassphrase} autocomplete="new-password" />
      </div>
      <div class="field">
        <label for="init-passphrase-confirm">Confirm passphrase</label>
        <input id="init-passphrase-confirm" type="password" bind:value={initPassphraseConfirm} autocomplete="new-password" />
      </div>
      <div class="warning-box">
        Write down your passphrase and keep it somewhere safe. Without it, your backups
        can never be restored — not by you, not by anyone.
      </div>
    {/if}

    <div class="form-actions">
      <button type="submit" class="btn btn-primary" disabled={initing || !repoForm.configured}>
        {initing ? 'Initializing...' : 'Create Repository'}
      </button>
    </div>

    {#if initResult}
      <div class="test-result" class:success={initResult.includes('success')} class:error={initResult.includes('failed') || initResult.includes('required') || initResult.includes('do not match')}>
        {initResult}
        {#if initHint}<span class="result-hint">{initHint}</span>{/if}
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

  .advanced-options {
    margin-top: var(--space-4);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    background: var(--color-bg);
  }

  .advanced-options summary {
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-sm);
    font-weight: 500;
  }

  .advanced-options[open] summary {
    color: var(--color-text);
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

  .warning-box {
    margin-top: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius-sm);
    background: var(--color-warning-muted);
    border: 1px solid var(--color-warning);
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    line-height: 1.5;
  }

  .warning-box strong {
    color: var(--color-warning);
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

  select {
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: var(--space-2) var(--space-3);
    color: var(--color-text);
    font-size: var(--text-sm);
  }

  select:focus {
    outline: none;
    border-color: var(--color-accent);
  }
</style>
