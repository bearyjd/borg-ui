import { invoke } from '@tauri-apps/api/core';

export interface RepoConfig {
  ssh_host: string;
  ssh_port: number;
  ssh_user: string;
  repo_path: string;
  ssh_key_path: string | null;
}

class RepoState {
  config = $state<RepoConfig | null>(null);
  connected = $state(false);

  get hasRepo(): boolean {
    return this.config !== null && this.config.ssh_host !== '';
  }

  async load(): Promise<RepoConfig | null> {
    const config = await invoke<RepoConfig | null>('load_repo_config');
    if (config) {
      this.config = config;
      this.connected = true;
    }
    return config;
  }

  async save(repo: RepoConfig): Promise<void> {
    await invoke('save_repo_config', { repo });
    this.config = repo;
    try {
      const ok = await invoke<boolean>('test_ssh_connection', {
        host: repo.ssh_host,
        port: repo.ssh_port,
        user: repo.ssh_user,
        keyPath: repo.ssh_key_path,
      });
      this.connected = ok;
    } catch {
      this.connected = false;
    }
  }
}

export const repoState = new RepoState();
