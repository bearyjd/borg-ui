import { invoke } from '@tauri-apps/api/core';
import type { RepoConfig } from './repo.svelte';
import type { ScheduleConfig } from './schedule.svelte';
import type { RetentionConfig } from './retention.svelte';

export interface Profile {
  id: string;
  name: string;
  repo: RepoConfig;
  backup_selection: BackupSelection;
  schedule: ScheduleConfig | null;
  integrity_schedule: { enabled: boolean } | null;
  restore_drill_schedule: { enabled: boolean } | null;
  resource_policy: ResourcePolicy;
  hardening: HardeningPosture;
  reporting: ReportPreferences;
  retention: RetentionConfig | null;
  archive_template: string | null;
  pre_backup: string | null;
  post_backup: string | null;
}

export interface ReportPreferences {
  enabled: boolean;
  webhook_enabled: boolean;
  smtp_enabled: boolean;
  smtp_host: string;
  smtp_port: number;
  smtp_tls_mode: 'start_tls' | 'implicit_tls';
  smtp_username: string;
  email_from: string;
  email_to: string;
  daily_digest: boolean;
  stale_after_hours: number;
  failure_threshold: number;
}

export interface HardeningPosture {
  append_only_declared: boolean;
  restricted_ssh_declared: boolean;
  encrypted_repository_declared: boolean;
  recovery_key_exported: boolean;
  server_maintenance_documented: boolean;
}

export interface ResourcePolicy {
  skip_on_battery: boolean;
  prevent_sleep: boolean;
  wake_for_backup: boolean;
  upload_limit_kib: number | null;
  allowed_wifi_names: string[];
  removable_destination_trigger: boolean;
}

export interface BackupSelection {
  source_paths: string[];
  excludes: string[];
  template_id: string | null;
  template_version: number | null;
}

export interface ProfilesData {
  schema_version: number;
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
