import { invoke } from '@tauri-apps/api/core';
import type { RepoConfig } from './repo.svelte';
import type { ScheduleConfig } from './schedule.svelte';
import type { RetentionConfig } from './retention.svelte';

export interface Profile {
  id: string;
  name: string;
  repo: RepoConfig;
  schedule: ScheduleConfig | null;
  retention: RetentionConfig | null;
  archive_template: string | null;
  pre_backup: string | null;
  post_backup: string | null;
}

export interface ProfilesData {
  profiles: Profile[];
  active_id: string | null;
}

class ProfilesState {
  profiles = $state<Profile[]>([]);
  activeId = $state<string | null>(null);
  loaded = $state(false);

  get active(): Profile | null {
    if (!this.activeId) return null;
    return this.profiles.find((p) => p.id === this.activeId) ?? null;
  }

  async load(): Promise<ProfilesData> {
    const data = await invoke<ProfilesData>('list_profiles');
    this.profiles = data.profiles;
    this.activeId = data.active_id;
    this.loaded = true;
    return data;
  }

  async setActive(id: string): Promise<void> {
    await invoke('set_active_profile', { id });
    this.activeId = id;
  }

  async create(name: string, repo: RepoConfig): Promise<Profile> {
    const profile = await invoke<Profile>('create_profile', { name, repo });
    await this.load();
    return profile;
  }

  async rename(id: string, name: string): Promise<void> {
    await invoke('rename_profile', { id, name });
    const p = this.profiles.find((p) => p.id === id);
    if (p) p.name = name;
  }

  async remove(id: string): Promise<void> {
    await invoke('delete_profile', { id });
    await this.load();
  }
}

export const profilesState = new ProfilesState();
