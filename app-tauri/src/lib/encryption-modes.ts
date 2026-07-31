/**
 * Whether creating a repository in a given borg encryption mode requires a
 * passphrase.
 *
 * This mirrors `encryption_needs_passphrase` in
 * `app-tauri/src-tauri/src/commands.rs`. The rule used to be written out twice —
 * once in Rust, once inline in InitRepoSection — and the two drifted: the
 * backend started requiring a passphrase for the `authenticated` modes while the
 * frontend still hid the passphrase field for them, so those two modes could not
 * be created at all. `init_repo` rejected every attempt with "passphrase required
 * for this encryption mode" and no control on screen could satisfy it.
 *
 * Keep this the single frontend definition, and keep it in step with the Rust
 * one — `encryption-modes.test.ts` pins the full mode list so a new mode cannot
 * be added on one side only.
 */
export const ENCRYPTION_MODES = [
  'none',
  'authenticated',
  'authenticated-blake2',
  'repokey',
  'keyfile',
  'repokey-blake2',
  'keyfile-blake2'
] as const;

export type EncryptionMode = (typeof ENCRYPTION_MODES)[number];

/**
 * Only `none` is genuinely passphrase-free. The `authenticated` modes do not
 * encrypt the data, but they still have a key protected by a passphrase —
 * verified against borg 1.4.4, where a repo created with an empty passphrase
 * opens ONLY with the empty one.
 */
export function encryptionNeedsPassphrase(mode: string): boolean {
  return mode !== 'none';
}
