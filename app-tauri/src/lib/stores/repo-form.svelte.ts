import type { RepoConfig } from './repo.svelte';

export type RepoType = 'ssh' | 'local';

/**
 * The repository form on the Settings page, shared between the Connection,
 * Initialize, and Passphrase sections (and read by Profiles/Retention).
 * Holds only the field values — per-section results, hints, and modals stay
 * in the section components.
 */
class RepoFormState {
  repoType = $state<RepoType>('ssh');
  sshHost = $state('');
  sshPort = $state(22);
  sshUser = $state('');
  repoPath = $state('');
  sshKeyPath = $state('');

  // For a local repo, "configured" means a folder path is filled in. For SSH,
  // we need host + user + path. Used to enable Save/Init/Prune/passphrase.
  get configured(): boolean {
    if (this.repoType === 'local') return this.repoPath.trim() !== '';
    return (
      this.sshHost.trim() !== '' &&
      this.sshUser.trim() !== '' &&
      this.repoPath.trim() !== ''
    );
  }

  /** Build a RepoConfig from the current form, honoring the repo type. */
  buildRepoConfig(): RepoConfig {
    if (this.repoType === 'local') {
      // The empty-host/empty-user convention IS the local marker — the backend
      // then uses repo_path directly as an on-disk path.
      return {
        ssh_host: '',
        ssh_port: 0,
        ssh_user: '',
        repo_path: this.repoPath,
        ssh_key_path: null,
      };
    }
    return {
      ssh_host: this.sshHost,
      ssh_port: this.sshPort,
      ssh_user: this.sshUser,
      repo_path: this.repoPath,
      ssh_key_path: this.sshKeyPath || null,
    };
  }

  currentRepoFromForm(): RepoConfig | null {
    if (!this.configured) return null;
    if (this.repoType === 'ssh' && !this.sshUser) return null;
    return this.buildRepoConfig();
  }
}

export const repoForm = new RepoFormState();
