import { invoke } from '@tauri-apps/api/core';

export interface ScheduleConfig {
  enabled: boolean;
  source_paths: string[];
  schedule: { type: 'hourly' } | { type: 'daily'; hour: number; minute: number };
  excludes: string[];
}

/** Short human label like "Daily at 02:00" or "Every hour". */
export function describeSchedule(config: ScheduleConfig): string {
  if (config.schedule.type === 'hourly') return 'Every hour';
  const hh = String(config.schedule.hour).padStart(2, '0');
  const mm = String(config.schedule.minute).padStart(2, '0');
  return `Daily at ${hh}:${mm}`;
}

/**
 * Compute the next scheduled run as a Date, or null when it can't be predicted.
 *
 * Daily runs at a fixed HH:MM (Task Scheduler `/SC DAILY /ST HH:MM`), so the
 * next run is exact. Hourly runs are registered with `/SC HOURLY /MO 1` and no
 * `/ST`, so Windows anchors them to whenever the task was registered — the exact
 * minute isn't knowable here. Return null for hourly rather than show a clock
 * time that could be up to ~59 min off; the UI falls back to the "Every hour"
 * label.
 */
export function nextRun(config: ScheduleConfig, from: Date = new Date()): Date | null {
  if (!config.enabled) return null;
  if (config.schedule.type === 'hourly') {
    return null;
  }
  const { hour, minute } = config.schedule;
  const next = new Date(from);
  next.setHours(hour, minute, 0, 0);
  if (next.getTime() <= from.getTime()) {
    next.setDate(next.getDate() + 1);
  }
  return next;
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
