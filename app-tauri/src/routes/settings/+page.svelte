<script lang="ts">
  import { repoForm } from '$lib/stores/repo-form.svelte';
  import { profilesState } from '$lib/stores/profiles.svelte';
  import ProfilesSection from '$lib/components/settings/ProfilesSection.svelte';
  import ArchiveNamingSection from '$lib/components/settings/ArchiveNamingSection.svelte';
  import CommandsSection from '$lib/components/settings/CommandsSection.svelte';
  import ConnectionSection from '$lib/components/settings/ConnectionSection.svelte';
  import InitRepoSection from '$lib/components/settings/InitRepoSection.svelte';
  import RepoPassphraseSection from '$lib/components/settings/RepoPassphraseSection.svelte';
  import NotificationsSection from '$lib/components/settings/NotificationsSection.svelte';
  import StartupSection from '$lib/components/settings/StartupSection.svelte';
  import ScheduleSection from '$lib/components/settings/ScheduleSection.svelte';
  import ResourcePolicySection from '$lib/components/settings/ResourcePolicySection.svelte';
  import HardeningSection from '$lib/components/settings/HardeningSection.svelte';
  import ReportingSection from '$lib/components/settings/ReportingSection.svelte';
  import SecondaryRepositorySection from '$lib/components/settings/SecondaryRepositorySection.svelte';
  import PlaceholderPolicySection from '$lib/components/settings/PlaceholderPolicySection.svelte';
  import StorageWarningSection from '$lib/components/settings/StorageWarningSection.svelte';
  import RetentionSection from '$lib/components/settings/RetentionSection.svelte';
  import DiagnosticsSection from '$lib/components/settings/DiagnosticsSection.svelte';
  import IntegritySection from '$lib/components/settings/IntegritySection.svelte';
  import RecoverySection from '$lib/components/settings/RecoverySection.svelte';
  import UpdateSection from '$lib/components/settings/UpdateSection.svelte';

  let passphraseSection = $state<RepoPassphraseSection | undefined>();
</script>

<div class="settings-page">
  <header class="page-header">
    <h1>Settings</h1>
    <p class="subtitle">Repository and connection configuration</p>
  </header>

  <ProfilesSection repoFromForm={() => repoForm.currentRepoFromForm()} repoType={() => repoForm.repoType} />

  {#if profilesState.active}
    <ArchiveNamingSection />
    <CommandsSection />
  {/if}

  <ConnectionSection />

  <InitRepoSection onInitialized={() => passphraseSection?.refresh()} />

  <RepoPassphraseSection bind:this={passphraseSection} />

  <RetentionSection getRepo={() => repoForm.buildRepoConfig()} repoConfigured={repoForm.configured} />
  {#if profilesState.active}<SecondaryRepositorySection />{/if}

  <NotificationsSection />

  <ScheduleSection />
  {#if profilesState.active}<ResourcePolicySection />{/if}
  {#if profilesState.active}
    <PlaceholderPolicySection />
    <StorageWarningSection />
  {/if}
  {#if profilesState.active}<HardeningSection />{/if}
  {#if profilesState.active}<ReportingSection />{/if}

  {#if profilesState.active}
    <IntegritySection />
    <RecoverySection />
  {/if}

  <StartupSection />

  <UpdateSection />

  <DiagnosticsSection />
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
</style>
