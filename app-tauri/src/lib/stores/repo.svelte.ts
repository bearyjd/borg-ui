import { invoke } from '@tauri-apps/api/core';

export interface RepoConfig {
  ssh_host: string;
  ssh_port: number;
  ssh_user: string;
  repo_path: string;
  ssh_key_path: string | null;
}

/**
 * A repo is "local" (a folder on this PC, a USB drive, or a network share)
 * when both the SSH host and user are empty. The backend uses repo_path
 * directly as an on-disk path in that case. Otherwise it is an SSH repo.
 */
export function isLocalRepo(config: Pick<RepoConfig, 'ssh_host' | 'ssh_user'>): boolean {
  return config.ssh_host.trim() === '' && config.ssh_user.trim() === '';
}

/** Human-readable description of where a repo lives, for status displays. */
export function describeRepo(config: RepoConfig): string {
  if (isLocalRepo(config)) {
    return `Local folder: ${config.repo_path}`;
  }
  return `${config.ssh_user}@${config.ssh_host}:${config.repo_path}`;
}

class RepoState {
  config = $state<RepoConfig | null>(null);
  connected = $state(false);

  get hasRepo(): boolean {
    if (this.config === null) return false;
    if (isLocalRepo(this.config)) {
      return this.config.repo_path.trim() !== '';
    }
    return this.config.ssh_host !== '';
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
    if (isLocalRepo(repo)) {
      // Local/USB/network-folder repos have no server to test against; a
      // saved config with a destination path counts as "connected".
      this.connected = repo.repo_path.trim() !== '';
      return;
    }
    try {
      // Resolves on success, rejects with ssh's stderr on failure.
      await invoke('test_ssh_connection', {
        host: repo.ssh_host,
        port: repo.ssh_port,
        user: repo.ssh_user,
        keyPath: repo.ssh_key_path,
      });
      this.connected = true;
    } catch {
      this.connected = false;
    }
  }
}

export const repoState = new RepoState();
