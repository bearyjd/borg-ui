<script lang="ts">
  import '../app.css';
  import { page } from '$app/stores';
  import { onMount } from 'svelte';
  import { isConnected, loadRepoConfig } from '$lib/stores/repo';

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

  let connected = $state(false);

  onMount(async () => {
    try {
      await loadRepoConfig();
    } catch (_) {}
  });

  $effect(() => {
    const unsub = isConnected.subscribe((v) => (connected = v));
    return unsub;
  });
</script>

<div class="app-shell">
  <nav class="sidebar">
    <div class="sidebar-brand">
      <span class="brand-icon">⬡</span>
      <span class="brand-text">BorgUI</span>
      <span class="brand-version">v0.1</span>
    </div>

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
        <span class="status-text">{connected ? 'Repo connected' : 'No repo connected'}</span>
      </div>
    </div>
  </nav>

  <main class="content">
    {@render children()}
  </main>
</div>

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
  }

  .content {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-8);
  }
</style>
