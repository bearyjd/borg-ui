import { describe, expect, test } from 'vitest';
import {
  PASSPHRASE_ROTATED_UNSAVED_PREFIX,
  PASSPHRASE_ROTATION_INDETERMINATE_PREFIX,
  passphraseFailureMessage,
  planPassphraseSave,
  type PassphraseSavePlan
} from './passphrase-save';

/** Convenience: the four inputs that drive the decision, with sane defaults. */
function req(over: Partial<Parameters<typeof planPassphraseSave>[0]> = {}) {
  return {
    hasStoredPassphrase: false,
    storeOnly: false,
    passphrase: 'hunter2',
    confirm: 'hunter2',
    ...over
  };
}

function expectSave(plan: PassphraseSavePlan) {
  if (plan.kind !== 'save') throw new Error(`expected a save plan, got: ${JSON.stringify(plan)}`);
  return plan;
}

describe('planPassphraseSave', () => {
  test('first-time set only stores the passphrase — there is nothing to rotate yet', () => {
    const plan = expectSave(planPassphraseSave(req({ hasStoredPassphrase: false })));
    expect(plan.mode).toBe('store');
    expect(plan.command).toBe('set_repo_passphrase');
  });

  // The whole point of the fix: with a passphrase already stored, "Change"
  // must rotate the REPOSITORY's passphrase, not silently overwrite the
  // stored copy and desync it from the repo.
  test('changing an existing passphrase rotates the repository passphrase', () => {
    const plan = expectSave(planPassphraseSave(req({ hasStoredPassphrase: true })));
    expect(plan.mode).toBe('rotate');
    expect(plan.command).toBe('change_repo_passphrase');
  });

  test('store-only repair overrides rotation for an already-stored passphrase', () => {
    const plan = expectSave(
      planPassphraseSave(req({ hasStoredPassphrase: true, storeOnly: true }))
    );
    expect(plan.mode).toBe('store');
    expect(plan.command).toBe('set_repo_passphrase');
  });

  // The checkbox is only rendered when a passphrase exists, but a stale
  // `storeOnly` must never turn a first-time set into something else.
  test('store-only with nothing stored is still a plain store', () => {
    const plan = expectSave(
      planPassphraseSave(req({ hasStoredPassphrase: false, storeOnly: true }))
    );
    expect(plan.mode).toBe('store');
    expect(plan.command).toBe('set_repo_passphrase');
  });

  test('rotate and store report different success text', () => {
    const rotate = expectSave(planPassphraseSave(req({ hasStoredPassphrase: true })));
    const store = expectSave(planPassphraseSave(req({ hasStoredPassphrase: false })));
    expect(rotate.successMessage).toMatch(/repository passphrase changed/i);
    expect(store.successMessage).not.toMatch(/repository passphrase changed/i);
  });

  test.each([
    ['empty passphrase', req({ passphrase: '', confirm: '' }), /cannot be empty/i],
    ['mismatched confirmation', req({ confirm: 'typo' }), /do not match/i],
    // An empty passphrase is reported as empty even when the confirmation
    // also differs — the more actionable message wins.
    ['empty beats mismatch', req({ passphrase: '', confirm: 'x' }), /cannot be empty/i]
  ])('rejects %s', (_label, input, expected) => {
    const plan = planPassphraseSave(input);
    expect(plan.kind).toBe('invalid');
    if (plan.kind !== 'invalid') return;
    expect(plan.message).toMatch(expected);
  });

  // Borg treats a passphrase literally, so whitespace is legal and must not be
  // trimmed away — trimming would lock users out of existing repositories.
  test('preserves whitespace-only passphrases instead of trimming them', () => {
    const plan = expectSave(planPassphraseSave(req({ passphrase: '  ', confirm: '  ' })));
    expect(plan.mode).toBe('store');
  });
});

describe('passphraseFailureMessage', () => {
  test('labels a failed rotation as a change, not a save', () => {
    expect(passphraseFailureMessage('rotate', 'boom')).toMatch(/Failed to change passphrase/);
  });

  test('labels a failed store as a save', () => {
    expect(passphraseFailureMessage('store', 'boom')).toMatch(/Failed to save passphrase/);
  });

  test('includes the underlying error text', () => {
    expect(passphraseFailureMessage('store', 'keyring locked')).toContain('keyring locked');
  });

  // The dangerous partial failure: borg accepted the new passphrase but the
  // keychain write failed. Prefixing that with "Failed to change passphrase"
  // would tell the user the opposite of what happened, so it passes through
  // verbatim. The backend emits this exact prefix (see the matching assertion
  // in app-tauri/src-tauri/src/commands.rs).
  test('passes the rotated-but-unsaved warning through unprefixed', () => {
    const backend = `${PASSPHRASE_ROTATED_UNSAVED_PREFIX} (keyring locked). Re-open this dialog...`;
    const message = passphraseFailureMessage('rotate', backend);
    expect(message).toBe(backend);
    expect(message).not.toMatch(/Failed to change passphrase/);
  });

  // A timeout does not mean "nothing happened": borg is deliberately not killed
  // (killing it mid-key-write risks destroying the key), so it may still commit
  // the rotation after we stop waiting.
  test('passes the timed-out/indeterminate warning through unprefixed', () => {
    const backend = `${PASSPHRASE_ROTATION_INDETERMINATE_PREFIX} (operation timed out after 120s). The stored copy was NOT updated...`;
    const message = passphraseFailureMessage('rotate', backend);
    expect(message).toBe(backend);
    expect(message).not.toMatch(/Failed to change passphrase/);
  });

  test('still prefixes an ordinary borg failure', () => {
    const message = passphraseFailureMessage('rotate', 'passphrase supplied is incorrect.');
    expect(message).toBe('Failed to change passphrase: passphrase supplied is incorrect.');
  });
});
