<script lang="ts">
  import { goto } from '$app/navigation';
  import { repoForm } from '$lib/stores/repo-form.svelte';
  import { repoState } from '$lib/stores/repo.svelte';
  import ConnectionSection from '$lib/components/settings/ConnectionSection.svelte';
  import InitRepoSection from '$lib/components/settings/InitRepoSection.svelte';
  import RepoPassphraseSection from '$lib/components/settings/RepoPassphraseSection.svelte';
  import ScheduleSection from '$lib/components/settings/ScheduleSection.svelte';

  const STEPS = [
    { title: 'Destination', blurb: 'Where backups are stored' },
    { title: 'Repository', blurb: 'Create or connect, set the passphrase' },
    { title: 'Schedule', blurb: 'Back up automatically' },
  ] as const;

  let step = $state(1);
  let passphraseSection = $state<RepoPassphraseSection | undefined>();

  // Step 1 → 2 requires the destination to actually be SAVED (repoState),
  // not just typed into the form — otherwise the wizard can "finish" with no
  // repository configured and the first backup fails. Later steps have no
  // hard requirement (connecting to an existing repo legitimately skips
  // Initialize). RepoPassphraseSection re-checks status in its own onMount.
  let canAdvance = $derived(step !== 1 || repoState.hasRepo);

  function next() {
    if (step < STEPS.length) step += 1;
  }

  function back() {
    if (step > 1) step -= 1;
  }

  function finish() {
    goto('/backup');
  }
</script>

<div class="setup-page">
  <header class="page-header">
    <h1>Set up your backups</h1>
    <p class="subtitle">Three steps and your PC is protected. You can change any of this later in Settings.</p>
  </header>

  <ol class="stepper" aria-label="Setup progress">
    {#each STEPS as s, i (s.title)}
      <li class="stepper-item" class:current={step === i + 1} class:done={step > i + 1} aria-current={step === i + 1 ? 'step' : undefined}>
        <span class="stepper-num">{step > i + 1 ? '✓' : i + 1}</span>
        <span class="stepper-text">
          <span class="stepper-title">{s.title}</span>
          <span class="stepper-blurb">{s.blurb}</span>
        </span>
      </li>
    {/each}
  </ol>

  {#if step === 1}
    <p class="step-intro">
      Pick where your backups should live, then use <strong>Verify &amp; save</strong> so you know the
      connection works before moving on. If the destination already holds a backup repository,
      <strong>Check repository</strong> will show what's in it.
    </p>
    <ConnectionSection />
  {:else if step === 2}
    <p class="step-intro">
      <strong>Brand-new destination?</strong> Create the repository below, and the passphrase you choose
      is saved automatically. <strong>Connecting to an existing backup?</strong> Skip “Create Repository”
      and just enter its passphrase in the second card.
    </p>
    <InitRepoSection onInitialized={() => passphraseSection?.refresh()} />
    <RepoPassphraseSection bind:this={passphraseSection} />
  {:else}
    <p class="step-intro">
      Set a schedule so backups happen without you thinking about them. You can also skip this and
      run backups manually from the Backup page.
    </p>
    <ScheduleSection />
  {/if}

  <div class="wizard-nav">
    <button type="button" class="btn btn-secondary" onclick={back} disabled={step === 1}>Back</button>
    {#if step < STEPS.length}
      <button type="button" class="btn btn-primary" onclick={next} disabled={!canAdvance}>
        Next: {STEPS[step].title}
      </button>
    {:else}
      <button type="button" class="btn btn-primary" onclick={finish}>
        Finish — choose what to back up
      </button>
    {/if}
    {#if step === 1 && !canAdvance}
      <span class="nav-hint">
        {repoForm.configured
          ? 'Use Verify & save (or Save) above to continue.'
          : 'Fill in the destination above, then Verify & save.'}
      </span>
    {/if}
  </div>
</div>

<style>
  .setup-page {
    max-width: 560px;
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
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

  .stepper {
    list-style: none;
    display: flex;
    gap: var(--space-3);
    padding: 0;
    margin: 0;
  }

  .stepper-item {
    flex: 1;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    background: var(--color-surface);
  }

  .stepper-item.current {
    border-color: var(--color-accent);
    background: var(--color-accent-muted);
  }

  .stepper-num {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 1.5rem;
    height: 1.5rem;
    flex-shrink: 0;
    border-radius: 50%;
    font-size: var(--text-xs);
    font-weight: 600;
    background: var(--color-surface-hover);
    color: var(--color-text-muted);
  }

  .stepper-item.current .stepper-num {
    background: var(--color-accent);
    color: var(--color-on-accent);
  }

  .stepper-item.done .stepper-num {
    background: var(--color-success-muted);
    color: var(--color-success);
  }

  .stepper-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .stepper-title {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text);
  }

  .stepper-item:not(.current) .stepper-title {
    color: var(--color-text-muted);
  }

  .stepper-blurb {
    font-size: 0.7rem;
    color: var(--color-text-dim);
    line-height: 1.3;
  }

  .step-intro {
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    line-height: 1.6;
  }

  .step-intro strong {
    color: var(--color-text);
  }

  .wizard-nav {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding-top: var(--space-2);
  }

  .nav-hint {
    color: var(--color-text-dim);
    font-size: var(--text-xs);
  }

  .btn {
    padding: var(--space-2) var(--space-4);
    /* app.css resets button borders; keep both buttons the same height even
       though only btn-secondary paints its border. */
    border: 1px solid transparent;
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

  @media (max-width: 620px) {
    .stepper {
      flex-direction: column;
    }
  }
</style>
