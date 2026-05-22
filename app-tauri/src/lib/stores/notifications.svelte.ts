import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from '@tauri-apps/plugin-notification';

const STORAGE_KEY = 'borgui.notifications.enabled';

class NotificationsState {
  enabled = $state(false);

  constructor() {
    if (typeof localStorage !== 'undefined') {
      this.enabled = localStorage.getItem(STORAGE_KEY) === 'true';
    }
  }

  async setEnabled(value: boolean): Promise<void> {
    if (value) {
      let granted = await isPermissionGranted();
      if (!granted) {
        const result = await requestPermission();
        granted = result === 'granted';
      }
      if (!granted) {
        throw new Error('Notification permission denied');
      }
    }
    this.enabled = value;
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(STORAGE_KEY, String(value));
    }
  }

  /**
   * Fire-and-forget notification. Safe to call without await — never throws,
   * never blocks the caller. Returns early if notifications are disabled or
   * the OS permission isn't granted. Errors are logged to console only.
   *
   * Keep `body` generic (no file paths, no raw error strings) — OS
   * notifications can persist in notification history and be visible on
   * shared screens.
   */
  async notify(title: string, body: string): Promise<void> {
    if (!this.enabled) return;
    try {
      const granted = await isPermissionGranted();
      if (granted) {
        sendNotification({ title, body });
      }
    } catch (e) {
      console.warn('Notification failed:', e);
    }
  }
}

export const notificationsState = new NotificationsState();
