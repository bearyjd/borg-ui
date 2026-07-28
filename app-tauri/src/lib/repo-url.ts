/**
 * Parse a combined repository address (the form Vorta and borg docs use)
 * into its parts, so a user who pastes `ssh://borg@192.168.1.12/backups/pc`
 * doesn't have to split it across four fields by hand.
 *
 * Accepted forms:
 *   ssh://user@host:port/path/to/repo
 *   ssh://user@host/path/to/repo
 *   user@host:/path/to/repo      (scp / borg legacy form)
 *   user@host:2222               (port, no path)
 *   user@host
 *
 * Returns null for anything else (e.g. a plain hostname) — the caller
 * should leave the field untouched in that case.
 */
export interface ParsedRepoUrl {
  host: string;
  user: string | null;
  port: number | null;
  path: string | null;
}

export function parseRepoUrl(input: string): ParsedRepoUrl | null {
  const raw = input.trim();
  if (raw === '') return null;
  const parsed = raw.toLowerCase().startsWith('ssh://')
    ? parseSshUrl(raw)
    : raw.includes('@')
      ? parseScpForm(raw)
      : null;
  // Never auto-fill a value that would read as a command-line option — the
  // backend's reject_option_like gate would refuse it later with a message
  // far from where it appeared. Refuse to parse instead.
  if (parsed && [parsed.host, parsed.user, parsed.path].some(isOptionLike)) return null;
  return parsed;
}

function isOptionLike(value: string | null): boolean {
  return value !== null && value.trimStart().startsWith('-');
}

function parseSshUrl(raw: string): ParsedRepoUrl | null {
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    return null;
  }
  if (url.hostname === '') return null;
  // borg treats `/./relative` and `/~/home-relative` prefixes specially —
  // keep the pathname exactly as written and let the server resolve it.
  const path = url.pathname && url.pathname !== '/' ? safeDecode(url.pathname) : null;
  // Hostnames are not percent-encoded; only decode the username and path.
  return {
    host: url.hostname,
    user: url.username ? safeDecode(url.username) : null,
    port: clampPort(url.port),
    path,
  };
}

function parseScpForm(raw: string): ParsedRepoUrl | null {
  const at = raw.lastIndexOf('@');
  const user = raw.slice(0, at);
  let host = raw.slice(at + 1);
  if (user === '' || host === '') return null;
  // IPv6 bracket syntax isn't supported end-to-end (the backend rejects ':'
  // in hostnames), so don't produce a garbage host from it.
  if (host.startsWith('[')) return null;

  let path: string | null = null;
  let port: number | null = null;
  const colon = host.indexOf(':');
  if (colon !== -1) {
    const after = host.slice(colon + 1);
    host = host.slice(0, colon);
    const portAndPath = after.match(/^(\d+)(\/.*)$/);
    if (portAndPath) {
      port = clampPort(portAndPath[1]);
      path = portAndPath[2];
    } else if (/^\d+$/.test(after)) {
      port = clampPort(after);
    } else if (after !== '') {
      path = after;
    }
  }
  if (host === '') return null;
  return { host, user, port, path };
}

function clampPort(value: string): number | null {
  if (!value) return null;
  const port = Number(value);
  return Number.isInteger(port) && port >= 1 && port <= 65535 ? port : null;
}

function safeDecode(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}
