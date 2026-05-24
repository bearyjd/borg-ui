import { invoke } from '@tauri-apps/api/core';

export interface BackupEvent {
  id: string;
  timestamp: string;
  kind: 'backup' | 'restore';
  archive_name: string;
  outcome: 'success' | 'failure';
  duration_seconds: number;
  file_count?: number;
  original_size?: number;
  error_message?: string;
}

class HistoryState {
  events = $state<BackupEvent[]>([]);
  loaded = $state(false);

  async load(): Promise<BackupEvent[]> {
    this.events = await invoke<BackupEvent[]>('load_backup_history');
    this.loaded = true;
    return this.events;
  }

  async record(event: BackupEvent): Promise<void> {
    await invoke('record_backup_event', { event });
    this.events = [...this.events, event];
  }

  async clear(): Promise<void> {
    await invoke('clear_backup_history');
    this.events = [];
  }
}

export const historyState = new HistoryState();
