import { describe, expect, test } from 'vitest';
import { buildRepoSummary } from './repo-info';

describe('buildRepoSummary', () => {
  test('reads stats from cache.stats', () => {
    const summary = buildRepoSummary(
      {
        encryption: { mode: 'repokey-blake2' },
        cache: { stats: { total_size: 100, total_csize: 80, unique_csize: 60 } },
      },
      []
    );
    expect(summary.encryption).toBe('repokey-blake2');
    expect(summary.totalSize).toBe(100);
    expect(summary.compressedSize).toBe(80);
    expect(summary.dedupSize).toBe(60);
  });

  test('falls back to repository.stats when cache.stats is absent', () => {
    const summary = buildRepoSummary(
      { repository: { stats: { total_size: 42 } } },
      []
    );
    expect(summary.totalSize).toBe(42);
    expect(summary.compressedSize).toBeNull();
  });

  test('missing stats and encryption degrade to null/unknown', () => {
    const summary = buildRepoSummary({}, []);
    expect(summary.encryption).toBe('unknown');
    expect(summary.totalSize).toBeNull();
    expect(summary.dedupSize).toBeNull();
  });

  test('empty archive list yields count 0 and no latest archive', () => {
    const summary = buildRepoSummary({}, []);
    expect(summary.archiveCount).toBe(0);
    expect(summary.latestArchive).toBeNull();
  });

  test('latest archive is picked by start timestamp, not list order', () => {
    const summary = buildRepoSummary({}, [
      { name: 'newest', start: '2026-07-28T09:00:00', id: 'c' },
      { name: 'oldest', start: '2026-07-01T09:00:00', id: 'a' },
      { name: 'middle', start: '2026-07-15T09:00:00', id: 'b' },
    ]);
    expect(summary.archiveCount).toBe(3);
    expect(summary.latestArchive).toEqual({ name: 'newest', start: '2026-07-28T09:00:00' });
  });
});
