<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import { repoState, isLocalRepo, type RepoConfig } from '$lib/stores/repo.svelte';
  import { profilesState } from '$lib/stores/profiles.svelte';
  import FieldHelp from '$lib/components/FieldHelp.svelte';
  import ProfilesSection from '$lib/components/settings/ProfilesSection.svelte';
  import ArchiveNamingSection from '$lib/components/settings/ArchiveNamingSection.svelte';
  import CommandsSection from '$lib/components/settings/CommandsSection.svelte';
  import NotificationsSection from '$lib/components/settings/NotificationsSection.svelte';
  import StartupSection from '$lib/components/settings/StartupSection.svelte';
  import ScheduleSection from '$lib/components/settings/ScheduleSection.svelte';
  import RetentionSection from '$lib/components/settings/RetentionSection.svelte';
  import DiagnosticsSection from '$lib/components/settings/DiagnosticsSection.svelte';
  import IntegritySection from '$lib/components/settings/IntegritySection.svelte';
  import RecoverySection from '$lib/components/settings/RecoverySection.svelte';

  type RepoType = 'ssh' | 'local';

  let repoType = $state<RepoType>('ssh');
  let sshHost = $state('');
  let sshPort = $state(22);
  let sshUser = $state('');
  let repoPath = $state('');
  let sshKeyPath = $state('');
  let testing = $state(false);
  let saving = $state(false);
  let testResult = $state('');
  let saveResult = $state('');
  let connectionStage = $state('');

  // Per-field pre-flight checks (Host reachability, SSH key validity).
  let hostCheckResult = $state('');
  let keyChecking = $state(false);
  let keyCheckResult = $state('');
  let keyPublicKey = $state('');
  let keyGenerating = $state(false);
  let overwriteKeyModalOpen = $state(false);
  let copyKeyResult = $state('');

  interface GeneratedSshKey {
    private_key_path: string;
    public_key: string;
  }

  // For a local repo, "configured" means a folder path is filled in. For SSH,
  // we need host + user + path. Used to enable Save/Init/Prune/passphrase.
  let repoConfigured = $derived(
    repoType === 'local'
      ? repoPath.trim() !== ''
      : sshHost.trim() !== '' && sshUser.trim() !== '' && repoPath.trim() !== ''
  );

  async function browseLocalRepoFolder() {
    const selected = await open({ directory: true, multiple: false, title: 'Select backup folder' });
    if (selected) repoPath = selected as string;
  }

  async function browseSshKey() {
    const selected = await open({
      directory: false,
      multiple: false,
      title: 'Select an unencrypted SSH private key',
    });
    if (!selected) return;
    sshKeyPath = selected as string;
    clearKeyResult();
    await checkKey();
  }

  async function generateSshKey(overwrite = false) {
    keyGenerating = true;
    keyCheckResult = '';
    copyKeyResult = '';
    try {
      const generated = await invoke<GeneratedSshKey>('generate_ssh_key', { overwrite });
      sshKeyPath = generated.private_key_path;
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

  async function copyPublicKey() {
    try {
      await navigator.clipboard.writeText(keyPublicKey);
      copyKeyResult = 'Copied.';
    } catch (e) {
      copyKeyResult = `Copy failed: ${e}`;
    }
  }

  function clearConnectionResults() {
    hostCheckResult = '';
    testResult = '';
    saveResult = '';
  }

  function clearKeyResult() {
    keyCheckResult = '';
    keyPublicKey = '';
    testResult = '';
    saveResult = '';
  }

  /** Build a RepoConfig from the current form, honoring the repo type. */
  function buildRepoConfig(): RepoConfig {
    if (repoType === 'local') {
      // The empty-host/empty-user convention IS the local marker — the backend
      // then uses repo_path directly as an on-disk path.
      return {
        ssh_host: '',
        ssh_port: 0,
        ssh_user: '',
        repo_path: repoPath,
        ssh_key_path: null,
      };
    }
    return {
      ssh_host: sshHost,
      ssh_port: sshPort,
      ssh_user: sshUser,
      repo_path: repoPath,
      ssh_key_path: sshKeyPath || null,
    };
  }

  function currentRepoFromForm(): RepoConfig | null {
    if (!repoConfigured) return null;
    if (repoType === 'ssh' && !sshUser) return null;
    return buildRepoConfig();
  }

  async function refreshPassphraseStatus() {
    const repo = currentRepoFromForm();
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

  function openPassphraseModal() {
    passphraseInput = '';
    passphraseConfirm = '';
    passphraseResult = '';
    passphraseModalOpen = true;
  }

  async function savePassphrase() {
    const repo = currentRepoFromForm();
    if (!repo) {
      passphraseResult = 'Configure SSH connection first.';
      return;
    }
    if (!passphraseInput) {
      passphraseResult = 'Passphrase cannot be empty.';
      return;
    }
    if (passphraseInput !== passphraseConfirm) {
      passphraseResult = 'Passphrases do not match.';
      return;
    }
    passphraseSaving = true;
    try {
      await invoke('set_repo_passphrase', { repo, passphrase: passphraseInput });
      hasPassphrase = true;
      passphraseModalOpen = false;
      passphraseInput = '';
      passphraseConfirm = '';
      passphraseResult = 'Passphrase saved to system keychain.';
    } catch (e) {
      passphraseResult = `Failed to save passphrase: ${e}`;
    } finally {
      passphraseSaving = false;
    }
  }

  let clearPassphraseModalOpen = $state(false);

  async function confirmClearPassphrase() {
    const repo = currentRepoFromForm();
    if (!repo) {
      clearPassphraseModalOpen = false;
      return;
    }
    try {
      await invoke('clear_repo_passphrase', { repo });
      hasPassphrase = false;
      passphraseResult = 'Passphrase removed from keychain.';
    } catch (e) {
      passphraseResult = `Failed to clear passphrase: ${e}`;
    } finally {
      clearPassphraseModalOpen = false;
    }
  }

  let hasPassphrase = $state(false);
  let passphraseLoading = $state(false);
  let passphraseModalOpen = $state(false);
  let passphraseInput = $state('');
  let passphraseConfirm = $state('');
  let passphraseSaving = $state(false);
  let passphraseResult = $state('');

  // Close the passphrase modals with the Escape key. Mirrors the
  // click-backdrop-to-close behaviour so modals are dismissable from the
  // keyboard too.
  $effect(() => {
    const anyOpen = clearPassphraseModalOpen || passphraseModalOpen || overwriteKeyModalOpen;
    if (!anyOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      clearPassphraseModalOpen = false;
      passphraseModalOpen = false;
      overwriteKeyModalOpen = false;
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  let initEncryption = $state<'repokey' | 'keyfile' | 'repokey-blake2' | 'keyfile-blake2' | 'authenticated' | 'authenticated-blake2' | 'none'>('repokey-blake2');
  let initPassphrase = $state('');
  let initPassphraseConfirm = $state('');
  let initing = $state(false);
  let initResult = $state('');
  let needsPassphrase = $derived(
    initEncryption !== 'none' &&
    initEncryption !== 'authenticated' &&
    initEncryption !== 'authenticated-blake2'
  );

  $effect(() => {
    const r = repoState.config;
    if (r) {
      repoType = isLocalRepo(r) ? 'local' : 'ssh';
      sshHost = r.ssh_host;
      sshPort = r.ssh_port || 22;
      sshUser = r.ssh_user;
      repoPath = r.repo_path;
      sshKeyPath = r.ssh_key_path ?? '';
    }
  });

  async function verifyAndSave() {
    testing = true;
    saving = true;
    testResult = '';
    saveResult = '';

    try {
      if (sshKeyPath && !(await checkKey())) return;
      if (!(await checkHost())) return;

      connectionStage = 'Signing in to the server…';
      await invoke('test_ssh_connection', {
        host: sshHost,
        port: sshPort,
        user: sshUser,
        keyPath: sshKeyPath || null,
      });
      connectionStage = 'Saving connection…';
      await repoState.save(buildRepoConfig(), { connectionVerified: true });
      testResult = 'Connection verified and saved.';
    } catch (e) {
      testResult = connectionStage === 'Saving connection…'
        ? `Connection worked, but settings could not be saved: ${e}`
        : `Could not sign in: ${e}`;
    } finally {
      connectionStage = '';
      testing = false;
      saving = false;
    }
  }

  async function checkHost(): Promise<boolean> {
    connectionStage = 'Checking server address…';
    hostCheckResult = '';
    try {
      await invoke('check_host_reachable', { host: sshHost, port: sshPort });
      hostCheckResult = `Server is reachable on port ${sshPort}.`;
      return true;
    } catch (e) {
      hostCheckResult = `Could not reach this server: ${e}`;
      return false;
    }
  }

  async function checkKey(): Promise<boolean> {
    connectionStage = 'Checking private key…';
    keyChecking = true;
    keyCheckResult = '';
    keyPublicKey = '';
    try {
      keyPublicKey = await invoke<string>('validate_ssh_key', { keyPath: sshKeyPath });
      keyCheckResult = 'Valid unencrypted private key.';
      return true;
    } catch (e) {
      keyCheckResult = `This key cannot be used: ${e}`;
      return false;
    } finally {
      connectionStage = '';
      keyChecking = false;
    }
  }

  async function save() {
    saving = true;
    saveResult = '';
    try {
      await repoState.save(buildRepoConfig());
      saveResult = 'Settings saved.';
    } catch (e) {
      saveResult = `Save failed: ${e}`;
    } finally {
      saving = false;
    }
  }

  async function initRepo() {
    initResult = '';
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
        repo: buildRepoConfig(),
        encryption: initEncryption,
        passphrase: needsPassphrase ? initPassphrase : null,
      });
      initResult = 'Repository initialized successfully.';
      initPassphrase = '';
      initPassphraseConfirm = '';
      await refreshPassphraseStatus();
    } catch (e) {
      initResult = `Init failed: ${e}`;
    } finally {
      initing = false;
    }
  }

  onMount(async () => {
    await refreshPassphraseStatus();
  });

  let lastActiveId = $state<string | null>(profilesState.activeId);
  $effect(() => {
    const id = profilesState.activeId;
    if (id === lastActiveId) return;
    lastActiveId = id;

    refreshPassphraseStatus();
  });
</script>

<div class="settings-page">
  <header class="page-header">
    <h1>Settings</h1>
    <p class="subtitle">Repository and connection configuration</p>
  </header>

  <ProfilesSection repoFromForm={currentRepoFromForm} repoType={() => repoType} />

  {#if profilesState.active}
    <ArchiveNamingSection />
    <CommandsSection />
  {/if}

  <form class="settings-form" onsubmit={(e) => { e.preventDefault(); repoType === 'local' ? save() : verifyAndSave(); }}>
    <fieldset class="form-group">
      <legend>Connection</legend>
      <FieldHelp text="Where should your backups be stored? Pick the kind of destination, then fill in the details below." />

      <div class="repo-type-toggle" role="radiogroup" aria-label="Repository type">
        <button
          type="button"
          class="repo-type-option"
          class:active={repoType === 'ssh'}
          role="radio"
          aria-checked={repoType === 'ssh'}
          onclick={() => (repoType = 'ssh')}
        >
          <span class="repo-type-title">Backup server (SSH)</span>
          <span class="repo-type-sub">A remote server you connect to over the internet.</span>
        </button>
        <button
          type="button"
          class="repo-type-option"
          class:active={repoType === 'local'}
          role="radio"
          aria-checked={repoType === 'local'}
          onclick={() => (repoType = 'local')}
        >
          <span class="repo-type-title">Local folder / USB / network drive</span>
          <span class="repo-type-sub">A folder on this PC, an external/USB drive, or a network share. No server needed.</span>
        </button>
      </div>

      {#if repoType === 'local'}
        <div class="field">
          <label for="local-path">Backup folder path</label>
          <div class="inline-row">
            <input id="local-path" type="text" bind:value={repoPath} placeholder="E:\Backups\her-pc" />
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
          <button type="submit" class="btn btn-primary" disabled={saving || !repoConfigured}>
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
              bind:value={sshHost}
              oninput={clearConnectionResults}
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
              bind:value={sshPort}
              oninput={clearConnectionResults}
              min="1"
              max="65535"
              inputmode="numeric"
              required
            />
          </div>
        </div>
        <div id="ssh-host-help">
          <FieldHelp text="Enter only the hostname or IP address. Keep port 22 unless your server provider gave you a different port." />
        </div>
        {#if hostCheckResult}
          <div class="field-result" role="status" class:success={hostCheckResult.startsWith('Server is')} class:error={hostCheckResult.startsWith('Could not')}>
            {hostCheckResult}
          </div>
        {/if}

        <div class="field">
          <label for="ssh-user">SSH username</label>
          <input
            id="ssh-user"
            type="text"
            bind:value={sshUser}
            oninput={clearConnectionResults}
            placeholder="borg"
            autocomplete="username"
            spellcheck="false"
            aria-describedby="ssh-user-help"
            required
          />
          <div id="ssh-user-help">
            <FieldHelp text="The login name provided by your server host. This is often “borg”, but it is not necessarily your Windows username." />
          </div>
        </div>

        <div class="field">
          <label for="repo-path">Repository folder on server</label>
          <input
            id="repo-path"
            type="text"
            bind:value={repoPath}
            oninput={clearConnectionResults}
            placeholder="/backups/her-pc"
            autocomplete="off"
            spellcheck="false"
            aria-describedby="repo-path-help"
            required
          />
          <div id="repo-path-help">
            <FieldHelp text="Use one folder for this PC. Enter the path your server provider gave you; do not include the server address or username." />
          </div>
        </div>

        <div class="field">
          <label for="ssh-key">Private key <span class="optional">(optional)</span></label>
          <div class="inline-row">
            <input
              id="ssh-key"
              type="text"
              bind:value={sshKeyPath}
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
            </div>
          {/if}
          {#if keyPublicKey}
            <details class="public-key" open={keyCheckResult.startsWith('New Ed25519')}>
              <summary>Show public key to add to the server</summary>
              <code>{keyPublicKey}</code>
              <div class="public-key-actions">
                <button type="button" class="btn btn-secondary" onclick={copyPublicKey}>Copy public key</button>
                {#if copyKeyResult}<span>{copyKeyResult}</span>{/if}
              </div>
            </details>
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
          </div>
        {/if}
      {/if}

      {#if saveResult}
        <div class="test-result" class:success={saveResult === 'Settings saved.'} class:error={saveResult.includes('failed')}>
          {saveResult}
        </div>
      {/if}
    </fieldset>
  </form>

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
        <button type="submit" class="btn btn-primary" disabled={initing || !repoConfigured}>
          {initing ? 'Initializing...' : 'Create Repository'}
        </button>
      </div>

      {#if initResult}
        <div class="test-result" class:success={initResult.includes('success')} class:error={initResult.includes('failed') || initResult.includes('required') || initResult.includes('do not match')}>
          {initResult}
        </div>
      {/if}
    </fieldset>
  </form>

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
        <button type="button" class="btn btn-primary" onclick={openPassphraseModal} disabled={!repoConfigured}>
          {hasPassphrase ? 'Change passphrase' : 'Set passphrase'}
        </button>
        {#if hasPassphrase}
          <button type="button" class="btn btn-secondary" onclick={() => (clearPassphraseModalOpen = true)}>
            Clear
          </button>
        {/if}
      </div>

      {#if passphraseResult && !passphraseModalOpen}
        <div class="test-result" class:success={passphraseResult.includes('saved') || passphraseResult.includes('removed')} class:error={passphraseResult.includes('Failed') || passphraseResult.includes('first') || passphraseResult.includes('match') || passphraseResult.includes('empty')}>
          {passphraseResult}
        </div>
      {/if}
    </fieldset>
  </form>

  <RetentionSection getRepo={buildRepoConfig} {repoConfigured} />

  <NotificationsSection />

  <ScheduleSection />

  {#if profilesState.active}
    <IntegritySection />
    <RecoverySection />
  {/if}

  <StartupSection />

  <DiagnosticsSection />

  {#if overwriteKeyModalOpen}
    <div class="modal-backdrop" onclick={() => (overwriteKeyModalOpen = false)} role="presentation">
      <div
        class="modal"
        onclick={(e) => e.stopPropagation()}
        onkeydown={() => {}}
        role="dialog"
        tabindex="-1"
        aria-modal="true"
        aria-labelledby="overwrite-key-title"
      >
        <h2 id="overwrite-key-title">Replace generated SSH key?</h2>
        <p>A BorgUI-managed SSH key already exists. Replacing it will prevent server access until you install the new public key on the server.</p>
        <div class="modal-actions">
          <button type="button" class="btn btn-secondary" onclick={() => (overwriteKeyModalOpen = false)}>Cancel</button>
          <button type="button" class="btn btn-delete-confirm" disabled={keyGenerating} onclick={() => generateSshKey(true)}>
            {keyGenerating ? 'Replacing…' : 'Replace key'}
          </button>
        </div>
      </div>
    </div>
  {/if}

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
        <p>Enter the passphrase used to encrypt this borg repository. It will be stored in your OS keychain.</p>
        <form onsubmit={(e) => { e.preventDefault(); savePassphrase(); }}>
          <div class="field">
            <label for="pass-input">Passphrase</label>
            <input id="pass-input" type="password" autocomplete="new-password" bind:value={passphraseInput} />
          </div>
          <div class="field">
            <label for="pass-confirm">Confirm</label>
            <input id="pass-confirm" type="password" autocomplete="new-password" bind:value={passphraseConfirm} />
          </div>
          {#if passphraseResult}
            <div class="test-result error">{passphraseResult}</div>
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
</div>

<style>
  .settings-page {
    max-width: 560px;
  }

  .page-header {
    margin-bottom: var(--space-8);
  }

  .page-header h1 {
    font-size: var(--text-3xl);
    font-weight: 700;
    letter-spacing: -0.03em;
  }

  .subtitle {
    color: var(--color-text-muted);
    margin-top: var(--space-1);
  }

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

  .settings-form + .settings-form {
    margin-top: var(--space-6);
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

  .public-key {
    margin-top: var(--space-2);
    color: var(--color-text-muted);
    font-size: var(--text-xs);
  }

  .public-key summary {
    width: fit-content;
    color: var(--color-accent);
    cursor: pointer;
  }

  .public-key code {
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

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-4);
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
