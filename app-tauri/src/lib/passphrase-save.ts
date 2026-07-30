/**
 * Decides what the passphrase dialog's Save button actually does.
 *
 * Setting a passphrase for the first time only writes it to the OS keychain.
 * *Changing* one must rotate the repository's real passphrase via
 * `borg key change-passphrase` — overwriting only the stored copy leaves the
 * keychain silently out of sync with the repository, and every later backup
 * fails to unlock it.
 */

/** What a save does: rotate the repository's passphrase, or only store one. */
export type PassphraseSaveMode = 'rotate' | 'store';

export interface PassphraseSaveRequest {
  /** Whether the OS keychain already holds a passphrase for this repository. */
  readonly hasStoredPassphrase: boolean;
  /** Repair escape hatch: overwrite the stored copy without touching the repo. */
  readonly storeOnly: boolean;
  readonly passphrase: string;
  readonly confirm: string;
}

export interface PassphraseSaveAction {
  readonly kind: 'save';
  readonly mode: PassphraseSaveMode;
  readonly command: 'change_repo_passphrase' | 'set_repo_passphrase';
  readonly successMessage: string;
}

export interface PassphraseSaveRejection {
  readonly kind: 'invalid';
  readonly message: string;
}

export type PassphraseSavePlan = PassphraseSaveAction | PassphraseSaveRejection;

/**
 * Prefix the backend uses when `borg key change-passphrase` succeeded but the
 * keychain write did not — the one failure that must never be reported as
 * "nothing happened". Kept byte-identical to the Rust constant in
 * `app-tauri/src-tauri/src/commands.rs`, which is asserted by a test there.
 */
export const PASSPHRASE_ROTATED_UNSAVED_PREFIX =
  'The repository passphrase was changed, but the stored copy could not be updated';

/**
 * Prefix the backend uses when the rotation timed out and its outcome is
 * genuinely unknown — borg is not killed on timeout (that would risk corrupting
 * the key), so it may still have committed the change. Mirrors
 * `PASSPHRASE_ROTATION_INDETERMINATE_PREFIX` in commands.rs.
 */
export const PASSPHRASE_ROTATION_INDETERMINATE_PREFIX =
  'The passphrase change timed out, so it may or may not have been applied';

/** Backend messages that are already complete, accurate accounts for the user. */
const SELF_CONTAINED_PREFIXES = [
  PASSPHRASE_ROTATED_UNSAVED_PREFIX,
  PASSPHRASE_ROTATION_INDETERMINATE_PREFIX
];

const ROTATE_SUCCESS = 'Repository passphrase changed; the stored copy was updated to match.';
const STORE_SUCCESS = 'Passphrase saved to system keychain.';

export function planPassphraseSave(request: PassphraseSaveRequest): PassphraseSavePlan {
  // Deliberately not trimmed: borg compares passphrases byte for byte, so a
  // leading or trailing space is a legitimate part of one.
  if (!request.passphrase) {
    return { kind: 'invalid', message: 'Passphrase cannot be empty.' };
  }
  if (request.passphrase !== request.confirm) {
    return { kind: 'invalid', message: 'Passphrases do not match.' };
  }
  const rotate = request.hasStoredPassphrase && !request.storeOnly;
  return rotate
    ? {
        kind: 'save',
        mode: 'rotate',
        command: 'change_repo_passphrase',
        successMessage: ROTATE_SUCCESS
      }
    : {
        kind: 'save',
        mode: 'store',
        command: 'set_repo_passphrase',
        successMessage: STORE_SUCCESS
      };
}

export function passphraseFailureMessage(mode: PassphraseSaveMode, error: unknown): string {
  const text = String(error);
  // Already a complete, accurate account of a partial or unknown outcome —
  // prefixing it with "Failed to change passphrase" would state the opposite of
  // the truth, and steer the user away from the recovery it describes.
  if (SELF_CONTAINED_PREFIXES.some((prefix) => text.startsWith(prefix))) return text;
  const verb = mode === 'rotate' ? 'change' : 'save';
  return `Failed to ${verb} passphrase: ${text}`;
}
