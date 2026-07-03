<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { profilesState, type ReportPreferences } from '$lib/stores/profiles.svelte';
  import FieldHelp from '$lib/components/FieldHelp.svelte';

  let settings = $state<ReportPreferences>({
    enabled: false,
    webhook_enabled: false,
    smtp_enabled: false,
    smtp_host: '',
    smtp_port: 587,
    smtp_tls_mode: 'start_tls',
    smtp_username: '',
    email_from: '',
    email_to: '',
    daily_digest: false,
    stale_after_hours: 48,
    failure_threshold: 1,
  });
  let webhookUrl = $state('');
  let smtpPassword = $state('');
  let secretStatus = $state({ webhook_configured: false, smtp_password_configured: false });
  let result = $state('');
  let saving = $state(false);

  onMount(async () => {
    settings = { ...settings, ...(profilesState.active?.reporting ?? {}) };
    secretStatus = await invoke('reporting_secret_status');
  });

  async function save() {
    saving = true;
    result = '';
    try {
      await invoke('save_reporting_settings', {
        settings,
        webhookUrl: webhookUrl || null,
        smtpPassword: smtpPassword || null,
      });
      webhookUrl = '';
      smtpPassword = '';
      await profilesState.load();
      secretStatus = await invoke('reporting_secret_status');
      result = 'Reporting settings saved.';
    } catch (error) {
      result = `Could not save reporting settings: ${error}`;
    } finally {
      saving = false;
    }
  }

  async function testReport() {
    result = 'Sending test report…';
    try {
      await invoke('send_test_report');
      result = 'Test report delivered.';
    } catch (error) {
      result = `Test report failed: ${error}`;
    }
  }
</script>

<fieldset class="form-group">
  <legend>Health reporting</legend>
  <FieldHelp text="Outbound reporting is opt-in. Webhook URLs and SMTP passwords are stored in Windows Credential Manager and are excluded from configuration exports and diagnostics." />
  <label><input type="checkbox" bind:checked={settings.enabled} /> Enable outbound health reports</label>
  {#if settings.enabled}
    <label><input type="checkbox" bind:checked={settings.webhook_enabled} /> HTTPS webhook</label>
    {#if settings.webhook_enabled}
      <label class="field">
        <span>Webhook URL {secretStatus.webhook_configured ? '(stored)' : ''}</span>
        <input type="password" bind:value={webhookUrl} placeholder={secretStatus.webhook_configured ? 'Leave blank to keep stored URL' : 'https://…'} />
      </label>
    {/if}

    <label><input type="checkbox" bind:checked={settings.smtp_enabled} /> Email via SMTP</label>
    {#if settings.smtp_enabled}
      <div class="grid">
        <label class="field"><span>SMTP host</span><input bind:value={settings.smtp_host} /></label>
        <label class="field"><span>Port</span><input type="number" min="1" max="65535" bind:value={settings.smtp_port} /></label>
        <label class="field">
          <span>TLS mode</span>
          <select bind:value={settings.smtp_tls_mode}>
            <option value="start_tls">STARTTLS</option>
            <option value="implicit_tls">Implicit TLS</option>
          </select>
        </label>
        <label class="field"><span>Username</span><input bind:value={settings.smtp_username} /></label>
        <label class="field"><span>Password {secretStatus.smtp_password_configured ? '(stored)' : ''}</span><input type="password" bind:value={smtpPassword} placeholder="Leave blank to keep stored password" /></label>
        <label class="field"><span>From</span><input type="email" bind:value={settings.email_from} /></label>
        <label class="field"><span>To</span><input type="email" bind:value={settings.email_to} /></label>
      </div>
    {/if}
    <label><input type="checkbox" bind:checked={settings.daily_digest} /> Send a daily digest</label>
    <div class="grid">
      <label class="field"><span>Backup stale after hours</span><input type="number" min="1" bind:value={settings.stale_after_hours} /></label>
      <label class="field"><span>Failures before red status</span><input type="number" min="1" bind:value={settings.failure_threshold} /></label>
    </div>
  {/if}
  <div class="actions">
    <button class="btn btn-primary" onclick={save} disabled={saving}>{saving ? 'Saving…' : 'Save reporting'}</button>
    {#if settings.enabled}<button class="btn btn-secondary" onclick={testReport}>Send test</button>{/if}
  </div>
  {#if result}<p>{result}</p>{/if}
</fieldset>

<style>
  .form-group { display: grid; gap: var(--space-3); }
  label { display: flex; align-items: center; gap: var(--space-2); color: var(--color-text-muted); }
  .field { align-items: stretch; flex-direction: column; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: var(--space-3); }
  input, select { padding: var(--space-2) var(--space-3); border: 1px solid var(--color-border); border-radius: var(--radius-md); background: var(--color-bg); color: var(--color-text); }
  .actions { display: flex; gap: var(--space-2); }
  p { margin: 0; color: var(--color-text-muted); }
</style>
