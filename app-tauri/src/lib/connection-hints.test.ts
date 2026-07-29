import { describe, expect, test } from 'vitest';
import { explainConnectionError, type HintContext } from './connection-hints';

const SSH: HintContext[] = ['ssh'];
const SSH_REPO: HintContext[] = ['ssh', 'repo'];
const REPO: HintContext[] = ['repo'];
const KEY: HintContext[] = ['key'];

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
  ])('%s → hint', (raw, contexts, expected) => {
    expect(explainConnectionError(raw, contexts)).toMatch(expected);
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
