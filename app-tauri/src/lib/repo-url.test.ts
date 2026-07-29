import { describe, expect, test } from 'vitest';
import { parseRepoUrl } from './repo-url';

describe('parseRepoUrl', () => {
  test.each([
    [
      'ssh://borg@192.168.1.12/backups/vorta-repos/laptop-backup',
      { host: '192.168.1.12', user: 'borg', port: null, path: '/backups/vorta-repos/laptop-backup' },
    ],
    [
      'ssh://borg@tower:2222/backups/pc',
      { host: 'tower', user: 'borg', port: 2222, path: '/backups/pc' },
    ],
    ['ssh://tower/backups/pc', { host: 'tower', user: null, port: null, path: '/backups/pc' }],
    ['borg@tower:/backups/pc', { host: 'tower', user: 'borg', port: null, path: '/backups/pc' }],
    ['borg@tower:2222/backups/pc', { host: 'tower', user: 'borg', port: 2222, path: '/backups/pc' }],
    ['borg@tower:2222', { host: 'tower', user: 'borg', port: 2222, path: null }],
    ['borg@tower', { host: 'tower', user: 'borg', port: null, path: null }],
    ['user@host:./backups/laptop', { host: 'host', user: 'user', port: null, path: './backups/laptop' }],
    ['  borg@tower  ', { host: 'tower', user: 'borg', port: null, path: null }],
  ])('parses %s', (input, expected) => {
    expect(parseRepoUrl(input)).toEqual(expected);
  });

  test.each([
    ['plain hostname', 'backup.example.com'],
    ['plain IP', '192.168.1.12'],
    ['empty', ''],
    ['bare scheme', 'ssh://'],
    ['empty user', '@tower'],
    ['empty host', 'borg@'],
    ['IPv6 bracket host (unsupported end-to-end)', 'borg@[::1]:/repo'],
  ])('returns null for %s', (_label, input) => {
    expect(parseRepoUrl(input)).toBeNull();
  });

  // Security: values that would read as command-line options must never be
  // auto-filled — mirrors the backend reject_option_like gate.
  test.each([
    ['percent-encoded option-like username', 'ssh://%2DoProxyCommand=calc@host/repo'],
    ['option-like host', 'user@-badhost'],
    ['option-like scp path', 'user@host:-oProxyCommand=calc'],
  ])('refuses %s', (_label, input) => {
    expect(parseRepoUrl(input)).toBeNull();
  });

  test('out-of-range port is dropped, not clamped to garbage', () => {
    expect(parseRepoUrl('ssh://borg@tower:99999/repo')).toBeNull();
    expect(parseRepoUrl('borg@tower:99999')).toEqual({
      host: 'tower',
      user: 'borg',
      port: null,
      path: null,
    });
  });
});
