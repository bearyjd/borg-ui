import { readdirSync, readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';
import { ENCRYPTION_MODES, encryptionNeedsPassphrase } from './encryption-modes';

describe('encryptionNeedsPassphrase', () => {
  test('only the unencrypted mode is passphrase-free', () => {
    expect(encryptionNeedsPassphrase('none')).toBe(false);
    for (const mode of ENCRYPTION_MODES.filter((m) => m !== 'none')) {
      expect(encryptionNeedsPassphrase(mode), mode).toBe(true);
    }
  });

  // The regression this file exists to prevent: the backend required a
  // passphrase for these while the frontend hid the field, so neither mode
  // could be created — init_repo rejected every attempt and no control on
  // screen could satisfy it.
  test('the authenticated modes require a passphrase', () => {
    expect(encryptionNeedsPassphrase('authenticated')).toBe(true);
    expect(encryptionNeedsPassphrase('authenticated-blake2')).toBe(true);
  });
});

describe('parity with the Rust definition', () => {
  const configRs = readFileSync(
    new URL('../../src-tauri/../../crates/borg-core/src/config.rs', import.meta.url),
    'utf8'
  );

  // A mode added to borg-core but not here would render with no passphrase
  // field (or the wrong one), which is exactly how the two drifted apart.
  test('covers every mode borg-core accepts', () => {
    const block = configRs.match(/VALID_ENCRYPTION_MODES[^=]*=\s*&\[([^\]]*)\]/);
    expect(block, 'VALID_ENCRYPTION_MODES not found in borg-core config.rs').not.toBeNull();
    const rustModes = [...block![1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
    expect([...rustModes].sort()).toEqual([...ENCRYPTION_MODES].sort());
  });

  test('matches the Rust passphrase rule', () => {
    // Scanned across the command modules rather than read from one hard-coded
    // path: this assertion is about the rule, not about which file currently
    // holds it, and pinning the filename made a pure code move look like a
    // behavior regression.
    const commandsDir = new URL('../../src-tauri/src/commands/', import.meta.url);
    const sources = readdirSync(commandsDir)
      .filter((name) => name.endsWith('.rs'))
      .map((name) => readFileSync(new URL(name, commandsDir), 'utf8'));

    // The Rust side is `mode != "none"`. If that ever gains another exempt
    // mode, this fails and the two must be reconciled deliberately.
    const rule = /fn encryption_needs_passphrase\(mode: &str\) -> bool \{\s*mode != "none"\s*\}/;
    const matches = sources.filter((source) => rule.test(source));
    expect(matches, 'encryption_needs_passphrase rule not found in src-tauri/src/commands/').toHaveLength(1);
  });
});
