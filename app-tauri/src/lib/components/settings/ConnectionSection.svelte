<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import { repoState, isLocalRepo } from '$lib/stores/repo.svelte';
  import { repoForm } from '$lib/stores/repo-form.svelte';
  import { parseRepoUrl, type ParsedRepoUrl } from '$lib/repo-url';
  import { explainConnectionError, type HintContext } from '$lib/connection-hints';
  import FieldHelp from '$lib/components/FieldHelp.svelte';
  import SshKeyOnboarding from './SshKeyOnboarding.svelte';
  import RepoSummaryPanel from './RepoSummaryPanel.svelte';
  import ReplaceKeyModal from './ReplaceKeyModal.svelte';
  import { loadRepoSummary, type RepoSummary } from '$lib/repo-info';

  let testing = $state(false);
  let saving = $state(false);
  let testResult = $state('');
  let saveResult = $state('');
  let connectionStage = $state('');

  // Plain-language hints derived from raw error text, shown under the
  // corresponding raw error so non-experts know what to try next.
  let testHint = $state('');
  let hostCheckHint = $state('');
  let keyCheckHint = $state('');
  // Set when a pasted ssh:// or user@host address was auto-split into fields.
  let autofillNote = $state('');

  // Per-field pre-flight checks (Host reachability, SSH key validity).
  let hostCheckResult = $state('');
  let keyChecking = $state(false);
  let keyCheckResult = $state('');
  let keyPublicKey = $state('');
  let keyGenerating = $state(false);
  let overwriteKeyModalOpen = $state(false);

  interface GeneratedSshKey {
    private_key_path: string;
    public_key: string;
  }

  async function browseLocalRepoFolder() {
    const selected = await open({ directory: true, multiple: false, title: 'Select backup folder' });
    if (selected) repoForm.repoPath = selected as string;
  }

  async function browseSshKey() {
    const selected = await open({
      directory: false,
      multiple: false,
      title: 'Select an unencrypted SSH private key',
    });
    if (!selected) return;
    repoForm.sshKeyPath = selected as string;
    clearKeyResult();
    await checkKey();
  }

  async function generateSshKey(overwrite = false) {
    keyGenerating = true;
    keyCheckResult = '';
    try {
      const generated = await invoke<GeneratedSshKey>('generate_ssh_key', { overwrite });
      repoForm.sshKeyPath = generated.private_key_path;
      keyPublicKey = generated.public_key;
      keyCheckResult = 'New Ed25519 key generated. Add the public key to your backup server.';
      overwriteKeyModalOpen = false;
    } catch (e) {
      const message = String(e);
      if (!overwrite && message.includes('already exists')) {
        overwriteKeyModalOpen = true;
      } else {
        keyCheckResult = `Could not generate key: ${message}`;
      }
    } finally {
      keyGenerating = false;
    }
  }

  function clearConnectionResults() {
    hostCheckResult = '';
    testResult = '';
    saveResult = '';
    testHint = '';
    hostCheckHint = '';
    autofillNote = '';
    repoSummary = null;
    repoCheckError = '';
    repoCheckHint = '';
    repoCheckWarning = '';
  }

  /** Plain-language suggestion for a raw ssh/borg error, or '' if unknown. */
  function hintFor(e: unknown, contexts: HintContext[]): string {
    return explainConnectionError(String(e), contexts) ?? '';
  }

  /** Hint contexts for operations on the configured repository. */
  function repoContexts(): HintContext[] {
    return repoForm.repoType === 'ssh' ? ['ssh', 'repo'] : ['repo'];
  }

  function applyParsedUrl(parsed: ParsedRepoUrl) {
    repoForm.sshHost = parsed.host;
    const filled: string[] = [];
    if (parsed.user) {
      repoForm.sshUser = parsed.user;
      filled.push('username');
    }
    if (parsed.port) {
      repoForm.sshPort = parsed.port;
      filled.push('port');
    }
    if (parsed.path) {
      repoForm.repoPath = parsed.path;
      filled.push('repository folder');
    }
    autofillNote = filled.length > 0
      ? `That looked like a full repository address — the ${filled.join(', ').replace(/, ([^,]*)$/, ' and $1')} ${filled.length > 1 ? 'were' : 'was'} filled in for you. Double-check the fields below.`
      : 'Simplified the pasted address to just the server name.';
  }

  function onHostPaste(e: ClipboardEvent) {
    const text = e.clipboardData?.getData('text') ?? '';
    const parsed = parseRepoUrl(text);
    if (!parsed) return;
    e.preventDefault();
    clearConnectionResults();
    applyParsedUrl(parsed);
  }

  // Catch typed-in (not pasted) combined addresses once the field loses focus,
  // so we never rewrite the value mid-keystroke.
  function onHostBlur() {
    const parsed = parseRepoUrl(repoForm.sshHost);
    if (parsed && parsed.host !== repoForm.sshHost.trim()) {
      clearConnectionResults();
      applyParsedUrl(parsed);
    }
  }

  let repoSummary = $state<RepoSummary | null>(null);
  let repoChecking = $state(false);
  let repoCheckError = $state('');
  let repoCheckHint = $state('');
  // Set when the repository itself was readable but the backup list wasn't —
  // the summary still renders, minus the archive rows.
  let repoCheckWarning = $state('');

  async function checkRepository() {
    repoChecking = true;
    repoCheckError = '';
    repoCheckHint = '';
    repoCheckWarning = '';
    repoSummary = null;
    try {
      const result = await loadRepoSummary(repoForm.buildRepoConfig());
      repoSummary = result.summary;
      repoCheckWarning = result.warning;
    } catch (e) {
      repoCheckError = `Could not read the repository: ${e}`;
      repoCheckHint = hintFor(e, repoContexts());
    } finally {
      repoChecking = false;
    }
  }

  $effect(() => {
    const r = repoState.config;
    if (r) {
      // Only discard the repository summary when the destination actually
      // changed — a save that writes back the same values (e.g. right after
      // Verify & save or a local Save) must not wipe a still-valid summary.
      const locationChanged =
        r.ssh_host !== repoForm.sshHost ||
        r.ssh_user !== repoForm.sshUser ||
        r.repo_path !== repoForm.repoPath ||
        (r.ssh_port || 22) !== repoForm.sshPort;
      repoForm.repoType = isLocalRepo(r) ? 'local' : 'ssh';
      repoForm.sshHost = r.ssh_host;
      repoForm.sshPort = r.ssh_port || 22;
      repoForm.sshUser = r.ssh_user;
      repoForm.repoPath = r.repo_path;
      repoForm.sshKeyPath = r.ssh_key_path ?? '';
      if (locationChanged) {
        repoSummary = null;
        repoCheckError = '';
        repoCheckHint = '';
        repoCheckWarning = '';
      }
    }
  });

  async function verifyAndSave() {
    testing = true;
    saving = true;
    testResult = '';
    saveResult = '';

    try {
      if (repoForm.sshKeyPath && !(await checkKey())) return;
      if (!(await checkHost())) return;

      connectionStage = 'Signing in to the server…';
      await invoke('test_ssh_connection', {
        host: repoForm.sshHost,
        port: repoForm.sshPort,
        user: repoForm.sshUser,
        keyPath: repoForm.sshKeyPath || null,
      });
      connectionStage = 'Saving connection…';
      await repoState.save(repoForm.buildRepoConfig(), { connectionVerified: true });
      testResult = 'Connection verified and saved.';
      testHint = '';
      connectionStage = 'Reading repository…';
      await checkRepository();
    } catch (e) {
      if (connectionStage === 'Saving connection…') {
        testResult = `Connection worked, but settings could not be saved: ${e}`;
        testHint = '';
      } else {
        testResult = `Could not sign in: ${e}`;
        testHint = hintFor(e, ['ssh', 'key']);
      }
    } finally {
      connectionStage = '';
      testing = false;
      saving = false;
    }
  }

  async function checkHost(): Promise<boolean> {
    connectionStage = 'Checking server address…';
    hostCheckResult = '';
    hostCheckHint = '';
    try {
      await invoke('check_host_reachable', { host: repoForm.sshHost, port: repoForm.sshPort });
      hostCheckResult = `Server is reachable on port ${repoForm.sshPort}.`;
      return true;
    } catch (e) {
      hostCheckResult = `Could not reach this server: ${e}`;
      hostCheckHint = hintFor(e, ['ssh']);
      return false;
    }
  }

  async function checkKey(): Promise<boolean> {
    connectionStage = 'Checking private key…';
    keyChecking = true;
    keyCheckResult = '';
    keyCheckHint = '';
    keyPublicKey = '';
    try {
      keyPublicKey = await invoke<string>('validate_ssh_key', { keyPath: repoForm.sshKeyPath });
      keyCheckResult = 'Valid unencrypted private key.';
      return true;
    } catch (e) {
      keyCheckResult = `This key cannot be used: ${e}`;
      keyCheckHint = hintFor(e, ['key']);
      return false;
    } finally {
      connectionStage = '';
      keyChecking = false;
    }
  }

  function clearKeyResult() {
    keyCheckResult = '';
    keyCheckHint = '';
    keyPublicKey = '';
    testResult = '';
    saveResult = '';
  }

  async function save() {
    saving = true;
    saveResult = '';
    try {
      await repoState.save(repoForm.buildRepoConfig());
      saveResult = 'Settings saved.';
    } catch (e) {
      saveResult = `Save failed: ${e}`;
    } finally {
      saving = false;
    }
  }
</script>

<form class="settings-form" onsubmit={(e) => { e.preventDefault(); repoForm.repoType === 'local' ? save() : verifyAndSave(); }}>
  <fieldset class="form-group">
    <legend>Connection</legend>
    <FieldHelp text="Where should your backups be stored? Pick the kind of destination, then fill in the details below." />

    <div class="repo-type-toggle" role="radiogroup" aria-label="Repository type">
      <button
        type="button"
        class="repo-type-option"
        class:active={repoForm.repoType === 'ssh'}
        role="radio"
        aria-checked={repoForm.repoType === 'ssh'}
        onclick={() => { repoForm.repoType = 'ssh'; clearConnectionResults(); }}
      >
        <span class="repo-type-title">Backup server (SSH)</span>
        <span class="repo-type-sub">A remote server you connect to over the internet.</span>
      </button>
      <button
        type="button"
        class="repo-type-option"
        class:active={repoForm.repoType === 'local'}
        role="radio"
        aria-checked={repoForm.repoType === 'local'}
        onclick={() => { repoForm.repoType = 'local'; clearConnectionResults(); }}
      >
        <span class="repo-type-title">Local folder / USB / network drive</span>
        <span class="repo-type-sub">A folder on this PC, an external/USB drive, or a network share. No server needed.</span>
      </button>
    </div>

    {#if repoForm.repoType === 'local'}
      <div class="field">
        <label for="local-path">Backup folder path</label>
        <div class="inline-row">
          <input id="local-path" type="text" bind:value={repoForm.repoPath} oninput={clearConnectionResults} placeholder="E:\Backups\her-pc" />
          <button type="button" class="btn btn-secondary" onclick={browseLocalRepoFolder}>Browse…</button>
        </div>
        <FieldHelp
          text="Back up to a folder on this PC, an external/USB drive, or a network share. Pick or type the folder where the backup should live."
          examples={[
            { input: 'E:\\Backups\\her-pc' },
            { input: '\\\\nas\\backups\\her-pc' },
          ]}
        />
      </div>

      <div class="form-actions">
        <button type="submit" class="btn btn-primary" disabled={saving || !repoForm.configured}>
          {saving ? 'Saving...' : 'Save'}
        </button>
      </div>
    {:else}
      <div class="field-row">
        <div class="field">
          <label for="ssh-host">Server address</label>
          <input
            id="ssh-host"
            type="text"
            bind:value={repoForm.sshHost}
            oninput={clearConnectionResults}
            onpaste={onHostPaste}
            onblur={onHostBlur}
            placeholder="backup.example.com"
            autocomplete="off"
            spellcheck="false"
            aria-describedby="ssh-host-help"
            required
          />
        </div>
        <div class="field field-sm">
          <label for="ssh-port">Port</label>
          <input
            id="ssh-port"
            type="number"
            bind:value={repoForm.sshPort}
            oninput={clearConnectionResults}
            min="1"
            max="65535"
            inputmode="numeric"
            required
          />
        </div>
      </div>
      <div id="ssh-host-help">
        <FieldHelp
          text="Enter the hostname or IP address. Keep port 22 unless your server provider gave you a different port. You can also paste a full address like ssh://borg@192.168.1.12/backups/laptop — the other fields fill in automatically."
          examples={[
            { input: 'backup.example.com' },
            { input: '192.168.1.12' },
          ]}
        />
      </div>
      {#if autofillNote}
        <div class="field-result info" role="status">{autofillNote}</div>
      {/if}
      {#if hostCheckResult}
        <div class="field-result" role="status" class:success={hostCheckResult.startsWith('Server is')} class:error={hostCheckResult.startsWith('Could not')}>
          {hostCheckResult}
          {#if hostCheckHint}<span class="result-hint">{hostCheckHint}</span>{/if}
        </div>
      {/if}

      <div class="field">
        <label for="ssh-user">SSH username</label>
        <input
          id="ssh-user"
          type="text"
          bind:value={repoForm.sshUser}
          oninput={clearConnectionResults}
          placeholder="borg"
          autocomplete="username"
          spellcheck="false"
          aria-describedby="ssh-user-help"
          required
        />
        <div id="ssh-user-help">
          <FieldHelp
            text="The login name on the backup server — not your Windows username. Your server provider tells you this."
            examples={[
              { input: 'borg' },
              { input: 'u384522' },
            ]}
          />
        </div>
      </div>

      <div class="field">
        <label for="repo-path">Repository folder on server</label>
        <input
          id="repo-path"
          type="text"
          bind:value={repoForm.repoPath}
          oninput={clearConnectionResults}
          placeholder="/backups/her-pc"
          autocomplete="off"
          spellcheck="false"
          aria-describedby="repo-path-help"
          required
        />
        <div id="repo-path-help">
          <FieldHelp
            text="Use one folder for this PC. Enter the path on the server; do not include the server address or username."
            examples={[
              { input: '/backups/her-pc' },
              { input: './backups/laptop' },
            ]}
          />
        </div>
      </div>

      <div class="field">
        <label for="ssh-key">Private key <span class="optional">(optional)</span></label>
        <div class="inline-row">
          <input
            id="ssh-key"
            type="text"
            bind:value={repoForm.sshKeyPath}
            oninput={clearKeyResult}
            placeholder="Use the default SSH key"
            autocomplete="off"
            spellcheck="false"
            aria-describedby="ssh-key-help"
          />
          <button type="button" class="btn btn-secondary" onclick={browseSshKey} disabled={keyChecking}>
            {keyChecking ? 'Checking…' : 'Browse…'}
          </button>
          <button type="button" class="btn btn-secondary" onclick={() => generateSshKey()} disabled={keyGenerating || keyChecking}>
            {keyGenerating ? 'Generating…' : 'Generate'}
          </button>
        </div>
        <div id="ssh-key-help">
          <FieldHelp text="Usually you can leave this blank. Select an existing unencrypted private key, or generate a dedicated Ed25519 key directly in BorgUI. Generation does not require Windows OpenSSH." />
        </div>
        {#if keyCheckResult}
          <div class="field-result" role="status" class:success={keyCheckResult.startsWith('Valid') || keyCheckResult.startsWith('New Ed25519')} class:error={keyCheckResult.startsWith('This key') || keyCheckResult.startsWith('Could not')}>
            {keyCheckResult}
            {#if keyCheckHint}<span class="result-hint">{keyCheckHint}</span>{/if}
          </div>
        {/if}
        {#if keyPublicKey}
          <SshKeyOnboarding publicKey={keyPublicKey} isNewKey={keyCheckResult.startsWith('New Ed25519')} />
        {/if}
      </div>

      <div class="form-actions connection-actions">
        <button type="submit" class="btn btn-primary" disabled={testing || saving}>
          {connectionStage || 'Verify & save'}
        </button>
        <span class="action-hint">Checks the server, key, and sign-in before saving.</span>
      </div>

      {#if testResult}
        <div class="test-result" role="status" class:success={testResult.includes('verified')} class:error={testResult.startsWith('Could not') || testResult.includes('could not be saved')}>
          {testResult}
          {#if testHint}<span class="result-hint">{testHint}</span>{/if}
        </div>
      {/if}
    {/if}

    {#if saveResult}
      <div class="test-result" class:success={saveResult === 'Settings saved.'} class:error={saveResult.includes('failed')}>
        {saveResult}
      </div>
    {/if}

    <div class="form-actions">
      <button type="button" class="btn btn-secondary" onclick={checkRepository} disabled={repoChecking || testing || saving || !repoForm.configured}>
        {repoChecking ? 'Checking repository…' : 'Check repository'}
      </button>
      <span class="action-hint">Reads the destination and shows what's stored there: encryption, size, and the latest backup.</span>
    </div>

    {#if repoCheckError}
      <div class="test-result error" role="status">
        {repoCheckError}
        {#if repoCheckHint}<span class="result-hint">{repoCheckHint}</span>{/if}
      </div>
    {/if}

    {#if repoSummary}
      <RepoSummaryPanel summary={repoSummary} warning={repoCheckWarning} />
    {/if}
  </fieldset>
</form>

<ReplaceKeyModal
  open={overwriteKeyModalOpen}
  busy={keyGenerating}
  onConfirm={() => generateSshKey(true)}
  onCancel={() => (overwriteKeyModalOpen = false)}
/>

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

  .inline-row {
    display: flex;
    gap: var(--space-2);
  }

  .inline-row input {
    flex: 1;
  }

  .repo-type-toggle {
    display: flex;
    gap: var(--space-3);
    margin-top: var(--space-4);
  }

  .repo-type-option {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    text-align: left;
    padding: var(--space-3) var(--space-4);
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    cursor: pointer;
    transition: all var(--duration-fast) var(--ease-out);
  }

  .repo-type-option:hover {
    border-color: var(--color-text-muted);
  }

  .repo-type-option.active {
    border-color: var(--color-accent);
    background: var(--color-accent-muted);
  }

  .repo-type-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .repo-type-option.active .repo-type-title {
    color: var(--color-accent);
  }

  .repo-type-sub {
    font-size: var(--text-xs);
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

  .optional {
    color: var(--color-text-dim);
    font-weight: 400;
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

  .field-row {
    display: flex;
    gap: var(--space-4);
  }

  .field-row .field {
    flex: 1;
  }

  .field-row .field-sm {
    flex: 0 0 100px;
  }

  .form-actions {
    display: flex;
    gap: var(--space-3);
    margin-top: var(--space-6);
  }

  .connection-actions {
    align-items: center;
  }

  .connection-actions .btn-primary {
    min-width: 9rem;
  }

  .action-hint {
    color: var(--color-text-dim);
    font-size: var(--text-xs);
    line-height: 1.4;
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

  /* Per-field check result (Host reachable / key valid). Monospace + wrapping
     so a long derived public key stays readable instead of overflowing. */
  .field-result {
    margin-top: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-all;
    background: var(--color-surface-hover);
    color: var(--color-text-muted);
  }

  .field-result.success {
    background: var(--color-success-muted);
    color: var(--color-success);
  }

  .field-result.error {
    background: var(--color-danger-muted);
    color: var(--color-danger);
  }

  .field-result.info {
    background: var(--color-accent-muted);
    color: var(--color-text-muted);
    font-family: inherit;
  }

  /* Plain-language suggestion shown under a raw error message. */
  .result-hint {
    display: block;
    margin-top: var(--space-2);
    font-family: inherit;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  @media (max-width: 620px) {
    .repo-type-toggle,
    .field-row {
      flex-direction: column;
    }

    .field-row .field-sm {
      flex-basis: auto;
    }

    .connection-actions {
      align-items: stretch;
      flex-direction: column;
    }
  }
</style>
