import { describe, expect, test } from 'vitest';
import { explainConnectionError, historyEventContexts, repoHintContexts, type HintContext } from './connection-hints';

const SSH: HintContext[] = ['ssh'];
const SSH_REPO: HintContext[] = ['ssh', 'repo'];
const REPO: HintContext[] = ['repo'];
const KEY: HintContext[] = ['key'];
const SSH_BACKUP: HintContext[] = ['ssh', 'repo', 'backup'];
const LOCAL_RESTORE: HintContext[] = ['repo', 'restore'];

describe('explainConnectionError', () => {
  test.each<[string, HintContext[], RegExp]>([
    ['borg@tower: Permission denied (publickey).', SSH, /authorized_keys/],
    ['Authentication failed.', SSH, /rejected the sign-in/],
    ['Host key verification failed.', SSH, /identity/],
    ['connect to host tower port 2222: Connection refused', SSH, /nothing is listening/],
    ['connection to tower timed out after 10s', SSH, /No answer from the server/],
    ['ssh: Could not resolve hostname towr', SSH, /couldn.t be looked up/],
    ['Connection reset by 192.168.1.12 port 22', SSH, /dropped the connection/],
    ['Load key "id_ed25519": key is encrypted', KEY, /password-protected/],
    ['A repository already exists at /backups/pc.', SSH_REPO, /skip Initialize/],
    ['Repository /backups/pc does not exist.', SSH_REPO, /Create Repository/],
    // borg's real lock message says "create/acquire" and "(timeout)" — must
    // hit the lock hint, not the network-timeout hint.
    ['Failed to create/acquire the lock /b/lock.exclusive (timeout).', SSH_REPO, /stale lock/],
    ['passphrase supplied in ... is incorrect.', REPO, /Repository Passphrase section/],
    ["bash: borg: command not found", SSH_REPO, /install BorgBackup/],
    // Backup/restore-specific hints.
    ['Error: VSS snapshot creation failed', SSH_BACKUP, /Volume Shadow Copy/],
    ['[Errno 28] No space left on device', SSH_BACKUP, /prune old backups/],
    // During a backup, a permission error is a source-file hint, not an
    // SSH sign-in hint — even for an SSH repository.
    ['C:\\Users\\x\\NTUSER.DAT: Permission denied', SSH_BACKUP, /antivirus/],
    ['Access is denied. (os error 5)', LOCAL_RESTORE, /folder you own/],
    ['stat: [Errno 2] No such file or directory: D:\\Photos', SSH_BACKUP, /source folder no longer exists/],
    // The very specific publickey failure still wins in backup context.
    ['borg@tower: Permission denied (publickey).', SSH_BACKUP, /authorized_keys/],
    // ssh's password-auth rejection is a sign-in hint even during a backup.
    ['borg@tower: Permission denied, please try again.', SSH_BACKUP, /rejected the sign-in/],
    // A missing borg binary is not a missing source folder.
    ['bash: /usr/bin/borg: No such file or directory', SSH_BACKUP, /install BorgBackup/],
    // Restore disk-full advice targets the LOCAL folder, not the repository.
    ['[Errno 28] No space left on device', LOCAL_RESTORE, /restoring into/],
    // A folder merely NAMED vss must not trigger snapshot advice.
    ['stat: [Errno 2] No such file or directory: D:\\vss-archive', SSH_BACKUP, /source folder no longer exists/],
    // Repo-level hints still reachable through backup/restore contexts.
    ['Repository /backups/pc does not exist.', SSH_BACKUP, /Create Repository/],
  ])('%s → hint', (raw, contexts, expected) => {
    expect(explainConnectionError(raw, contexts)).toMatch(expected);
  });

  test('repoHintContexts adds ssh only for remote repos', () => {
    expect(repoHintContexts(true, ['backup'])).toEqual(['ssh', 'repo', 'backup']);
    expect(repoHintContexts(false)).toEqual(['repo']);
  });

  describe('historyEventContexts', () => {
    test('maps known event kinds, degrades unknown kinds to repo-only', () => {
      expect(historyEventContexts('backup')).toEqual(['repo', 'backup']);
      expect(historyEventContexts('restore')).toEqual(['repo', 'restore']);
      expect(historyEventContexts('prune')).toEqual(['repo']);
    });

    test('repo-level failures still get a hint', () => {
      expect(
        explainConnectionError(
          'Backup failed: Failed to create/acquire the lock (timeout).',
          historyEventContexts('backup')
        )
      ).toMatch(/stale lock/);
    });

    // History events don't record their transport, so ssh-specific hints
    // must never fire — a timeout from a USB backup would otherwise get
    // "check the server address" advice.
    test('never emits ssh-specific hints for historical events', () => {
      expect(
        explainConnectionError(
          'Backup failed: connection to tower timed out after 10s',
          historyEventContexts('backup')
        )
      ).toBeNull();
    });
  });

  test.each<[string, string, HintContext[]]>([
    // A local-folder permission error must never get SSH public-key advice.
    ['local permission denied', 'Permission denied (os error 13)', REPO],
    // A missing key file must not get "Create Repository" advice.
    ['missing key file', 'key file does not exist', KEY],
    ['unknown error', 'something completely novel went wrong', SSH_REPO],
    ['empty', '', SSH_REPO],
  ])('returns null for %s', (_label, raw, contexts) => {
    expect(explainConnectionError(raw, contexts)).toBeNull();
  });
});
