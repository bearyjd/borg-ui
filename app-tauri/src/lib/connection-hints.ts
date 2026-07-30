/**
 * Translate raw ssh/borg failure text into a plain-language next step.
 * The raw error stays visible (it's what you'd paste into a search engine);
 * the hint tells a non-expert what to actually try.
 *
 * Hints are scoped by context so, e.g., a local-folder "permission denied"
 * never gets SSH-public-key advice:
 *   'ssh'     — reaching / signing in to a remote server
 *   'key'     — validating a local private-key file
 *   'repo'    — borg operations against an (already reachable) repository
 *   'backup'  — creating an archive (source-file reads, VSS snapshots)
 *   'restore' — extracting an archive to a local folder
 */
export type HintContext = 'ssh' | 'key' | 'repo' | 'backup' | 'restore';

/**
 * Contexts for an operation against a configured repository: always 'repo',
 * plus 'ssh' when the repo is remote, plus any operation-specific extras.
 */
export function repoHintContexts(isSsh: boolean, extra: HintContext[] = []): HintContext[] {
  return isSsh ? ['ssh', 'repo', ...extra] : ['repo', ...extra];
}

/**
 * Contexts for a recorded history event, so failure rows in the dashboard can
 * carry the same plain-language hints as the live operation surfaces.
 *
 * Deliberately NEVER includes 'ssh': the history store is global across
 * profiles and events don't record which transport they used, so an ssh hint
 * keyed off the currently-active profile could be confidently wrong for an
 * event from a different profile. Fewer hints beats wrong hints. `kind` is
 * accepted as a plain string because the Rust side stores an open String —
 * unknown kinds degrade to repo-level hints only.
 */
export function historyEventContexts(kind: string): HintContext[] {
  const extra: HintContext[] =
    kind === 'backup' || kind === 'restore' ? [kind] : [];
  return ['repo', ...extra];
}

interface Hint {
  pattern: RegExp;
  contexts: HintContext[];
  hint: string;
}

// Order matters: the first matching pattern wins, so put the most
// specific messages before the generic ones.
const HINTS: Hint[] = [
  {
    pattern: /permission denied \(publickey/i,
    contexts: ['ssh'],
    hint: 'The server refused the key. Make sure the public key is installed in authorized_keys for this exact username, and that the username is spelled right.',
  },
  {
    // Sign-in-specific spellings must beat the backup/restore permission
    // entries below — "please try again" is ssh's password-auth rejection.
    pattern: /permission denied, please try again|authentication failed/i,
    contexts: ['ssh'],
    hint: 'The server rejected the sign-in. Double-check the username and make sure the public key is installed on the server for that user.',
  },
  {
    // Before the generic ssh permission entry: during a backup, a
    // permission error is far more likely a source file than a sign-in.
    pattern: /permission denied|access is denied/i,
    contexts: ['backup'],
    hint: 'A file or folder couldn’t be read. This is usually Windows permissions or antivirus on one of the selected folders — the raw message above names the path; consider excluding it.',
  },
  {
    pattern: /permission denied|access is denied/i,
    contexts: ['restore'],
    hint: 'Windows blocked writing to the restore folder. Restore into a folder you own (like Documents or Downloads), or run BorgUI as administrator.',
  },
  {
    pattern: /permission denied|authentication failed/i,
    contexts: ['ssh'],
    hint: 'The server rejected the sign-in. Double-check the username and make sure the public key shown above is installed on the server for that user.',
  },
  {
    // Anchored to the platform crate's real messages ("VSS failed", "VSS
    // snapshot failed", "VSS unavailable") — a bare /vss/ would also match
    // backed-up paths like D:\vss-archive.
    pattern: /vss[^:]{0,20}fail|vss (not )?availab|vss unavailable|unexpected vss|volume shadow|shadow ?copy/i,
    contexts: ['backup'],
    hint: 'Windows couldn’t take a file snapshot (VSS). Try again; if it keeps failing, check that the “Volume Shadow Copy” service is running and that BorgUI has administrator rights.',
  },
  {
    // Restore-first: during a restore the full disk is the LOCAL target
    // folder — pruning the repository would free nothing.
    pattern: /no space left|not enough (disk )?space|disk full|insufficient disk space/i,
    contexts: ['restore'],
    hint: 'The folder you’re restoring into is out of space. Free some space there, or restore to a different drive or folder.',
  },
  {
    pattern: /no space left|not enough (disk )?space|disk full|insufficient disk space/i,
    contexts: ['repo', 'backup'],
    hint: 'The destination is out of space. Free some space there, or prune old backups (Retention section in Settings) and run Compact, then try again.',
  },
  {
    // Before the source-missing entry: "bash: /usr/bin/borg: No such file
    // or directory" is a missing borg install, not a missing source folder.
    pattern: /borg.*(not found|no such file)|command not found/i,
    contexts: ['ssh', 'repo', 'backup', 'restore'],
    hint: 'The borg program wasn’t found on the server. Ask your server provider to install BorgBackup, or check that it’s on the PATH for SSH sessions.',
  },
  {
    pattern: /no such file or directory|path does not exist|file not found/i,
    contexts: ['backup'],
    hint: 'A selected source folder no longer exists (it may have been moved, renamed, or be on a disconnected drive). Review the folder list on the Backup page.',
  },
  {
    pattern: /host key verification failed|remote host identification has changed/i,
    contexts: ['ssh'],
    hint: 'The server’s identity doesn’t match what this PC saw before. If the server was reinstalled or its address was reused this is expected — otherwise stop and check with whoever runs the server.',
  },
  {
    pattern: /connection refused/i,
    contexts: ['ssh'],
    hint: 'The machine answered, but nothing is listening on that port. Check the port number — SSH is usually 22 — and that the SSH service is running on the server.',
  },
  {
    // Before the network-timeout hint: borg's lock error also says "timeout"
    // ("Failed to create/acquire the lock ... (timeout)").
    pattern: /failed to \S+ the lock|lock(ing)? (timeout|failed)/i,
    contexts: ['repo'],
    hint: 'Another backup or borg command is (or was) using this repository. Wait for it to finish; if nothing else is running, the previous run may have crashed and left a stale lock.',
  },
  {
    pattern: /timed out/i,
    contexts: ['ssh'],
    hint: 'No answer from the server. Check the address for typos, confirm the server is on, and check whether a firewall or VPN is in the way.',
  },
  {
    pattern: /no route to host|network is unreachable/i,
    contexts: ['ssh'],
    hint: 'This PC can’t reach that network. Check your internet or VPN connection, and that the address is right for the network you’re on.',
  },
  {
    pattern: /could not resolve|name or service not known|nodename nor servname|getaddrinfo|temporary failure in name resolution/i,
    contexts: ['ssh'],
    hint: 'That name couldn’t be looked up. Check the server address for typos; if it’s a machine on your home network, try its IP address (like 192.168.1.12) instead.',
  },
  {
    pattern: /connection (reset|closed) by/i,
    contexts: ['ssh'],
    hint: 'The server dropped the connection mid-handshake. This often means SSH logins aren’t allowed for that user, or a security tool on the server blocked the attempt.',
  },
  {
    pattern: /key.*(encrypted|passphrase)|passphrase.*key/i,
    contexts: ['ssh', 'key'],
    hint: 'That private key is password-protected, which BorgUI can’t use for unattended backups. Use the Generate button to create a dedicated key instead.',
  },
  {
    pattern: /repository already exists|already exists at/i,
    contexts: ['repo'],
    hint: 'There’s already a backup repository at this destination — you can skip Initialize and just set the repository passphrase below.',
  },
  {
    pattern: /does not exist|is not a valid repository/i,
    contexts: ['repo'],
    hint: 'No repository exists at this destination yet. Use “Create Repository” in the Initialize section to set one up first.',
  },
  {
    pattern: /passphrase supplied.*incorrect|wrong passphrase|decryption failed|integrityerror/i,
    contexts: ['repo'],
    hint: 'The stored passphrase doesn’t match this repository. Use the Repository Passphrase section to enter the passphrase this repository was created with.',
  },
];

export function explainConnectionError(
  raw: string,
  contexts: HintContext[]
): string | null {
  for (const { pattern, contexts: hintContexts, hint } of HINTS) {
    if (!hintContexts.some((c) => contexts.includes(c))) continue;
    if (pattern.test(raw)) return hint;
  }
  return null;
}
