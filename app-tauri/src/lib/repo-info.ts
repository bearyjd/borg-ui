import { invoke } from '@tauri-apps/api/core';
import type { RepoConfig } from './stores/repo.svelte';

/** Vorta-style "what's actually in this repository" summary. */
export interface RepoSummary {
  encryption: string;
  totalSize: number | null;
  compressedSize: number | null;
  dedupSize: number | null;
  archiveCount: number;
  latestArchive: { name: string; start: string } | null;
}

interface BorgStats {
  total_size?: number;
  total_csize?: number;
  unique_csize?: number;
}

interface BorgInfoPayload {
  encryption?: { mode?: string };
  cache?: { stats?: BorgStats };
  // Some borg versions report stats here instead of under cache — the
  // backend (get_repo_info) reads both locations too.
  repository?: { stats?: BorgStats };
}

interface ArchiveEntry {
  name: string;
  start: string;
  id: string;
}

export interface RepoSummaryResult {
  summary: RepoSummary;
  /** Set when the repo was readable but the archive list wasn't. */
  warning: string;
}

/** Pure parse step, separated from the IPC calls so it's unit-testable. */
export function buildRepoSummary(
  info: BorgInfoPayload,
  archives: ArchiveEntry[]
): RepoSummary {
  const stats = info.cache?.stats ?? info.repository?.stats;
  // Don't trust list order for "latest" — pick by start timestamp
  // (ISO 8601 strings, so lexicographic comparison is chronological).
  const latest = archives.length > 0
    ? archives.reduce((a, b) => (a.start > b.start ? a : b))
    : null;
  return {
    encryption: info.encryption?.mode ?? 'unknown',
    totalSize: stats?.total_size ?? null,
    compressedSize: stats?.total_csize ?? null,
    dedupSize: stats?.unique_csize ?? null,
    archiveCount: archives.length,
    latestArchive: latest ? { name: latest.name, start: latest.start } : null,
  };
}

/**
 * Read what's actually in a repository. Throws if the repository itself is
 * unreadable; a failure listing archives is downgraded to `warning` so the
 * info half of the summary still renders.
 */
export async function loadRepoSummary(repo: RepoConfig): Promise<RepoSummaryResult> {
  // Sequential on purpose: each call spawns borg, which takes an exclusive
  // repository lock — concurrent calls would just contend on it.
  const info = await invoke<BorgInfoPayload>('get_repo_info', { repo });
  let archives: ArchiveEntry[] = [];
  let warning = '';
  try {
    archives = await invoke<ArchiveEntry[]>('list_archives', { repo });
  } catch (e) {
    warning = `Repository found, but its backup list could not be read: ${e}`;
  }
  return { summary: buildRepoSummary(info, archives), warning };
}
