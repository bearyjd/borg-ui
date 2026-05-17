import { writable, derived } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

export interface RepoConfig {
  ssh_host: string;
  ssh_port: number;
  ssh_user: string;
  repo_path: string;
  ssh_key_path: string | null;
}

export const repoConfig = writable<RepoConfig | null>(null);
export const isConnected = writable(false);

export const hasRepo = derived(repoConfig, ($repo) => $repo !== null && $repo.ssh_host !== '');

export async function loadRepoConfig(): Promise<RepoConfig | null> {
  const config = await invoke<RepoConfig | null>('load_repo_config');
  if (config) {
    repoConfig.set(config);
    isConnected.set(true);
  }
  return config;
}

export async function saveRepoConfig(repo: RepoConfig): Promise<void> {
  await invoke('save_repo_config', { repo });
  repoConfig.set(repo);
  isConnected.set(true);
}
