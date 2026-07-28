/**
 * Translate raw ssh/borg failure text into a plain-language next step.
 * The raw error stays visible (it's what you'd paste into a search engine);
 * the hint tells a non-expert what to actually try.
 *
 * Hints are scoped by context so, e.g., a local-folder "permission denied"
 * never gets SSH-public-key advice:
 *   'ssh'  — reaching / signing in to a remote server
 *   'key'  — validating a local private-key file
 *   'repo' — borg operations against an (already reachable) repository
 */
export type HintContext = 'ssh' | 'key' | 'repo';

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
    pattern: /permission denied|authentication failed/i,
    contexts: ['ssh'],
    hint: 'The server rejected the sign-in. Double-check the username and make sure the public key shown above is installed on the server for that user.',
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
  {
    pattern: /borg.*(not found|no such file)|command not found/i,
    contexts: ['ssh', 'repo'],
    hint: 'The borg program wasn’t found on the server. Ask your server provider to install BorgBackup, or check that it’s on the PATH for SSH sessions.',
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
