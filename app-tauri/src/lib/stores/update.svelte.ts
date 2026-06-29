import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';

export type UpdateStatus = 'idle' | 'checking' | 'available' | 'current' | 'installing' | 'error';

class UpdateState {
  status = $state<UpdateStatus>('idle');
  version = $state('');
  notes = $state('');
  error = $state('');
  downloaded = $state(0);
  total = $state<number | null>(null);
  private pending: Update | null = null;

  async check(): Promise<void> {
    if (this.status === 'checking' || this.status === 'installing') return;
    this.status = 'checking';
    this.error = '';
    try {
      await this.pending?.close();
      this.pending = await check({ timeout: 30_000 });
      if (this.pending) {
        this.version = this.pending.version;
        this.notes = this.pending.body ?? '';
        this.status = 'available';
      } else {
        this.version = '';
        this.notes = '';
        this.status = 'current';
      }
    } catch (error) {
      this.status = 'error';
      this.error = readableUpdateError(error);
    }
  }

  async install(): Promise<void> {
    if (!this.pending || this.status !== 'available') return;
    this.status = 'installing';
    this.downloaded = 0;
    this.total = null;
    try {
      await this.pending.downloadAndInstall((event) => {
        if (event.event === 'Started') this.total = event.data.contentLength ?? null;
        if (event.event === 'Progress') this.downloaded += event.data.chunkLength;
      }, { timeout: 10 * 60_000 });
      await relaunch();
    } catch (error) {
      this.status = 'error';
      this.error = readableUpdateError(error);
    }
  }

  dismiss(): void {
    if (this.status === 'available') this.status = 'idle';
  }
}

export function readableUpdateError(error: unknown): string {
  const text = String(error);
  if (/network|dns|connect|offline|timed? out/i.test(text)) {
    return 'Could not reach the update service. Check your connection and try again.';
  }
  if (/signature|public key|verify/i.test(text)) {
    return 'The update signature could not be verified. Nothing was installed.';
  }
  if (/json|manifest|response/i.test(text)) {
    return 'The update service returned an invalid manifest. Nothing was installed.';
  }
  return `Update failed: ${text}`;
}

export const updateState = new UpdateState();
