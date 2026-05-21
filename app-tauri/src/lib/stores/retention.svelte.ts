import { invoke } from '@tauri-apps/api/core';

export interface RetentionConfig {
  keep_hourly: number | null;
  keep_daily: number | null;
  keep_weekly: number | null;
  keep_monthly: number | null;
  keep_yearly: number | null;
}

class RetentionState {
  config = $state<RetentionConfig | null>(null);
  loaded = $state(false);

  async load(): Promise<RetentionConfig | null> {
    const config = await invoke<RetentionConfig | null>('load_retention_config');
    this.config = config;
    this.loaded = true;
    return config;
  }

  async save(config: RetentionConfig): Promise<void> {
    await invoke('save_retention_config', { config });
    this.config = config;
  }
}

export const retentionState = new RetentionState();
