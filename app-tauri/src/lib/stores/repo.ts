import { writable } from 'svelte/store';

export interface RepoConfig {
  ssh_host: string;
  ssh_port: number;
  ssh_user: string;
  repo_path: string;
  ssh_key_path: string | null;
}

export const repoConfig = writable<RepoConfig | null>(null);
export const isConnected = writable(false);
