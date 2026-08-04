import { describe, expect, test } from 'vitest';
import { eventEnvelope } from './history-event';

describe('eventEnvelope', () => {
  const now = Date.UTC(2026, 7, 4, 12, 30, 15); // 2026-08-04T12:30:15Z

  test('stamps id, timestamp, and duration from one instant', () => {
    // The regression this guards: three separate clock reads per event meant
    // the id, the timestamp, and the duration could disagree.
    const envelope = eventEnvelope(now - 90_000, now);

    expect(envelope.id).toBe(`${now}`);
    expect(envelope.timestamp).toBe('2026-08-04T12:30:15.000Z');
    expect(envelope.duration_seconds).toBe(90);
    expect(new Date(envelope.timestamp).getTime()).toBe(Number(envelope.id));
  });

  test('rounds the duration to whole seconds', () => {
    expect(eventEnvelope(now - 1400, now).duration_seconds).toBe(1);
    expect(eventEnvelope(now - 1600, now).duration_seconds).toBe(2);
  });

  test('reports zero for an operation that finished within the same second', () => {
    expect(eventEnvelope(now, now).duration_seconds).toBe(0);
  });

  test('defaults to the current clock when no instant is supplied', () => {
    const before = Date.now();
    const envelope = eventEnvelope(before);
    const after = Date.now();

    expect(Number(envelope.id)).toBeGreaterThanOrEqual(before);
    expect(Number(envelope.id)).toBeLessThanOrEqual(after);
  });
});
