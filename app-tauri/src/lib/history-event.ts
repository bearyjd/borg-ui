import type { BackupEvent } from './stores/history.svelte';

/** The identity/timing fields every recorded history event carries.
 *
 * The clock is read once and threaded through all three fields. The four call
 * sites this replaces each read it three times — `Date.now()` for the id, a
 * separate `new Date()` for the timestamp, and a third `Date.now()` for the
 * duration — so a single event could be stamped from three different instants.
 *
 * `now` is injectable so the envelope can be asserted without faking timers.
 */
export function eventEnvelope(
  startMs: number,
  now: number = Date.now()
): Pick<BackupEvent, 'id' | 'timestamp' | 'duration_seconds'> {
  return {
    id: `${now}`,
    timestamp: new Date(now).toISOString(),
    duration_seconds: Math.round((now - startMs) / 1000),
  };
}
