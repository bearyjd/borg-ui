<script lang="ts">
  import { repoForm } from '$lib/stores/repo-form.svelte';

  interface Props {
    publicKey: string;
    /** Freshly generated key vs. an existing one picked via Browse. */
    isNewKey: boolean;
  }

  let { publicKey, isNewKey }: Props = $props();

  let copyKeyResult = $state('');
  let copyInstallCommandResult = $state('');
  let copyVerifyCommandResult = $state('');

  function shellQuote(value: string) {
    return `'${value.replaceAll("'", "'\\''")}'`;
  }

  let authorizedKeysPath = $derived(repoForm.sshUser.trim() ? `~/.ssh/authorized_keys for ${repoForm.sshUser.trim()}` : '~/.ssh/authorized_keys');
  let installKeyCommand = $derived(
    publicKey
      ? `mkdir -p ~/.ssh && chmod 700 ~/.ssh && printf '%s\\n' ${shellQuote(publicKey.trim())} >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys`
      : ''
  );
  let verifySshCommand = $derived(
    repoForm.sshHost.trim() && repoForm.sshUser.trim()
      ? `ssh${repoForm.sshKeyPath.trim() ? ` -i "${repoForm.sshKeyPath.trim()}"` : ''} -p ${repoForm.sshPort || 22} ${repoForm.sshUser.trim()}@${repoForm.sshHost.trim()} "echo ok"`
      : ''
  );

  async function copyText(text: string): Promise<string> {
    try {
      await navigator.clipboard.writeText(text);
      return 'Copied.';
    } catch (e) {
      return `Copy failed: ${e}`;
    }
  }
</script>

<section class="ssh-onboarding" aria-label="SSH public key onboarding">
  <div class="onboarding-header">
    <h3>{isNewKey ? 'Install this new public key' : 'Use this existing public key'}</h3>
    <p>BorgUI does not ask for your server password and will not install keys for you. Add this public key on the backup server, then run Verify & save.</p>
  </div>

  <div class="onboarding-step">
    <h4>1. Copy the exact public key</h4>
    <code class="copy-block">{publicKey}</code>
    <div class="public-key-actions">
      <button type="button" class="btn btn-secondary" onclick={async () => (copyKeyResult = await copyText(publicKey))}>Copy public key</button>
      {#if copyKeyResult}<span>{copyKeyResult}</span>{/if}
    </div>
  </div>

  <div class="onboarding-step">
    <h4>2. Add it to the server account</h4>
    <p>On the server, append the key to <code>{authorizedKeysPath}</code>. The <code>.ssh</code> directory should be <code>700</code>; <code>authorized_keys</code> should be <code>600</code>.</p>
    {#if installKeyCommand}
      <code class="copy-block">{installKeyCommand}</code>
      <div class="public-key-actions">
        <button type="button" class="btn btn-secondary" onclick={async () => (copyInstallCommandResult = await copyText(installKeyCommand))}>Copy server command</button>
        {#if copyInstallCommandResult}<span>{copyInstallCommandResult}</span>{/if}
      </div>
    {/if}
  </div>

  {#if verifySshCommand}
    <div class="onboarding-step">
      <h4>3. Verify access</h4>
      <p>Use BorgUI’s Verify & save button below, or run this command from a terminal to confirm the server accepts the key:</p>
      <code class="copy-block">{verifySshCommand}</code>
      <div class="public-key-actions">
        <button type="button" class="btn btn-secondary" onclick={async () => (copyVerifyCommandResult = await copyText(verifySshCommand))}>Copy verification command</button>
        {#if copyVerifyCommandResult}<span>{copyVerifyCommandResult}</span>{/if}
      </div>
    </div>
  {/if}
</section>

<style>
  .ssh-onboarding {
    margin-top: var(--space-2);
    padding: var(--space-3);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    color: var(--color-text-muted);
    font-size: var(--text-xs);
  }

  .onboarding-header h3 {
    margin: 0 0 var(--space-1);
    color: var(--color-text);
    font-size: var(--text-sm);
  }

  .onboarding-header p,
  .onboarding-step p {
    margin: 0;
    line-height: 1.5;
  }

  .onboarding-step {
    margin-top: var(--space-3);
  }

  .onboarding-step h4 {
    margin: 0 0 var(--space-1);
    color: var(--color-text);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .copy-block {
    display: block;
    margin-top: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text-muted);
    font-size: 0.7rem;
    line-height: 1.5;
    overflow-wrap: anywhere;
    user-select: text;
  }

  .public-key-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
</style>
