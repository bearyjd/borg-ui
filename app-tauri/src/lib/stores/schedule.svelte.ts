import { invoke } from '@tauri-apps/api/core';

export interface ScheduleConfig {
  enabled: boolean;
  source_paths: string[];
  schedule: { type: 'hourly' } | { type: 'daily'; hour: number; minute: number };
  excludes: string[];
}

class ScheduleState {
  config = $state<ScheduleConfig | null>(null);
  loaded = $state(false);

  get enabled(): boolean {
    return this.config?.enabled ?? false;
  }

  async load(): Promise<ScheduleConfig | null> {
    const config = await invoke<ScheduleConfig | null>('load_schedule_config');
    this.config = config;
    this.loaded = true;
    return config;
  }

  async save(config: ScheduleConfig): Promise<void> {
    await invoke('save_schedule_config', { config });
    this.config = config;
  }
}

export const scheduleState = new ScheduleState();
