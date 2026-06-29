<script lang="ts">
  import '../app.css';
  import { page } from '$app/stores';
  import { onMount, onDestroy } from 'svelte';
  import { goto } from '$app/navigation';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { repoState, describeRepo } from '$lib/stores/repo.svelte';
  import { scheduleState } from '$lib/stores/schedule.svelte';
  import { retentionState } from '$lib/stores/retention.svelte';
  import { profilesState } from '$lib/stores/profiles.svelte';
  import { updateState } from '$lib/stores/update.svelte';

  interface Props {
    children: import('svelte').Snippet;
  }

  let { children }: Props = $props();

  const navItems = [
    { href: '/', label: 'Dashboard', icon: '◉' },
    { href: '/backup', label: 'Backup', icon: '↑' },
    { href: '/archives', label: 'Archives', icon: '▤' },
    { href: '/settings', label: 'Settings', icon: '⚙' },
  ];

  let connected = $derived(repoState.hasRepo);
  let repoSummary = $derived(repoState.config ? describeRepo(repoState.config) : '');
  let unlistenTray: UnlistenFn | undefined;

  async function switchProfile(id: string) {
    if (!id || id === profilesState.activeId) return;
    try {
      await profilesState.setActive(id);
      await Promise.all([
        repoState.load(),
        scheduleState.load().catch(() => {}),
        retentionState.load().catch(() => {}),
      ]);
    } catch (e) {
      console.error('Failed to switch profile:', e);
    }
  }

  onMount(async () => {
    try {
      await profilesState.load();
    } catch (e) {
      console.warn('Failed to load profiles:', e);
    }

    try {
      await repoState.load();
    } catch (e) {
      if (!String(e).includes('not found') && !String(e).includes('NotFound')) {
        console.error('Failed to load repo config:', e);
      }
    }

    try {
      unlistenTray = await listen('tray-trigger-backup', () => {
        goto('/backup');
      });
    } catch (e) {
      console.warn('Failed to subscribe to tray events:', e);
    }

    await updateState.check();
  });

  onDestroy(() => {
    unlistenTray?.();
  });
</script>

<div class="app-shell">
  <nav class="sidebar">
    <div class="sidebar-brand">
      <span class="brand-icon">⬡</span>
      <span class="brand-text">BorgUI</span>
      <span class="brand-version">v0.1</span>
    </div>

    {#if profilesState.profiles.length > 0}
      <div class="profile-picker">
        <label for="profile-select" class="profile-label">Profile</label>
        <select
          id="profile-select"
          value={profilesState.activeId ?? ''}
          onchange={(e) => switchProfile(e.currentTarget.value)}
        >
          {#each profilesState.profiles as p (p.id)}
            <option value={p.id}>{p.name}</option>
          {/each}
        </select>
      </div>
    {/if}

    <ul class="nav-list">
      {#each navItems as item}
        <li>
          <a
            href={item.href}
            class="nav-item"
            class:active={$page.url.pathname === item.href}
          >
            <span class="nav-icon">{item.icon}</span>
            <span class="nav-label">{item.label}</span>
          </a>
        </li>
      {/each}
    </ul>

    <div class="sidebar-footer">
      <div class="connection-status">
        <span class="status-dot" class:connected></span>
        <span class="status-text" title={connected ? repoSummary : ''}>
          {connected ? repoSummary : 'No repo connected'}
        </span>
      </div>
    </div>
  </nav>

  <main class="content">
    {@render children()}
  </main>
</div>

{#if updateState.status === 'available' || updateState.status === 'installing'}
  <div class="modal-backdrop" role="presentation">
    <div class="update-modal" role="dialog" aria-modal="true" aria-labelledby="update-title" tabindex="-1">
      <h2 id="update-title">BorgUI {updateState.version} is available</h2>
      {#if updateState.notes}
        <div class="release-notes">{updateState.notes}</div>
      {:else}
        <p>No release notes were provided.</p>
      {/if}
      <p>The signed update will only be downloaded and installed after you confirm. BorgUI may restart or close while Windows finishes installation.</p>
      {#if updateState.status === 'installing'}
        <p>Downloading… {updateState.total ? `${Math.round(updateState.downloaded / updateState.total * 100)}%` : ''}</p>
      {:else}
        <div class="modal-actions">
          <button class="btn btn-secondary" type="button" onclick={() => updateState.dismiss()}>Not now</button>
          <button class="btn btn-primary" type="button" onclick={() => updateState.install()}>Download and install</button>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .app-shell {
    display: flex;
    height: 100vh;
  }

  .sidebar {
    width: 220px;
    background: var(--color-surface);
    border-right: 1px solid var(--color-border-subtle);
    display: flex;
    flex-direction: column;
    padding: var(--space-4) 0;
    flex-shrink: 0;
  }

  .sidebar-brand {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-4) var(--space-6);
  }

  .profile-picker {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: 0 var(--space-4) var(--space-4);
  }

  .profile-label {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--color-text-dim);
    font-weight: 600;
  }

  .profile-picker select {
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: var(--space-2) var(--space-3);
    color: var(--color-text);
    font-size: var(--text-sm);
    width: 100%;
  }

  .profile-picker select:focus {
    outline: none;
    border-color: var(--color-accent);
  }

  .brand-icon {
    font-size: var(--text-2xl);
    color: var(--color-accent);
  }

  .brand-text {
    font-size: var(--text-lg);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .brand-version {
    font-size: var(--text-xs);
    color: var(--color-text-dim);
    background: var(--color-surface-hover);
    padding: 1px 6px;
    border-radius: var(--radius-sm);
  }

  .nav-list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0 var(--space-2);
    flex: 1;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-md);
    color: var(--color-text-muted);
    transition: all var(--duration-fast) var(--ease-out);
    text-decoration: none;
  }

  .nav-item:hover {
    background: var(--color-surface-hover);
    color: var(--color-text);
  }

  .nav-item.active {
    background: var(--color-accent-muted);
    color: var(--color-accent);
  }

  .nav-icon {
    font-size: var(--text-lg);
    width: 24px;
    text-align: center;
  }

  .nav-label {
    font-size: var(--text-sm);
    font-weight: 500;
  }

  .sidebar-footer {
    padding: var(--space-3) var(--space-4);
    border-top: 1px solid var(--color-border-subtle);
  }

  .connection-status {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--color-text-dim);
    transition: background var(--duration-normal) var(--ease-out);
  }

  .status-dot.connected {
    background: var(--color-success);
  }

  .status-text {
    font-size: var(--text-xs);
    color: var(--color-text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .content {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-8);
  }

  .modal-backdrop { position: fixed; inset: 0; z-index: 200; background: var(--color-backdrop); display: grid; place-items: center; }
  .update-modal { width: min(560px, 90vw); max-height: 80vh; overflow: auto; background: var(--color-surface); border: 1px solid var(--color-border); border-radius: var(--radius-lg); padding: var(--space-6); }
  .update-modal h2 { margin-bottom: var(--space-3); }
  .update-modal p, .release-notes { color: var(--color-text-muted); white-space: pre-wrap; margin-top: var(--space-3); }
  .modal-actions { display: flex; justify-content: flex-end; gap: var(--space-2); margin-top: var(--space-6); }
</style>
