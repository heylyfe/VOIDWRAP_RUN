// Pure parsing for the join-a-friend address field. Accepts a bare address
// or host:port (scheme optional, default http), or an explicit http(s) URL.
// Ports default to the game port; output is always scheme://host:port.
export const DEFAULT_JOIN_PORT = 7879;

const SCHEME_PREFIX = /^([a-zA-Z][a-zA-Z0-9+.-]*):\/\//;
const HOST_LABEL = /^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$/;
const IPV4 = /^\d+(\.\d+){3}$/;

function isValidHost(host: string): boolean {
  if (host.length === 0 || host.length > 253) {
    return false;
  }
  const labels = host.split('.');
  for (const label of labels) {
    if (!HOST_LABEL.test(label)) {
      return false;
    }
  }
  if (IPV4.test(host)) {
    for (const label of labels) {
      const octet = Number.parseInt(label, 10);
      if (octet > 255) {
        return false;
      }
    }
  }
  return true;
}

/**
 * Parse a friend's address into scheme://host:port, or null when it is not
 * an http(s) host[:port]. Surrounding whitespace is trimmed; any inner
 * whitespace, path, query, fragment, userinfo, IPv6 literal or non-http(s)
 * scheme is rejected. A missing port defaults to DEFAULT_JOIN_PORT.
 */
export function parseJoinAddress(text: string): string | null {
  const trimmed = text.trim();
  if (trimmed.length === 0) {
    return null;
  }
  if (/\s/.test(trimmed)) {
    return null;
  }

  let scheme = 'http';
  let rest = trimmed;
  const schemeMatch = SCHEME_PREFIX.exec(trimmed);
  if (schemeMatch) {
    const candidate = (schemeMatch[1] ?? '').toLowerCase();
    if (candidate !== 'http' && candidate !== 'https') {
      return null;
    }
    scheme = candidate;
    rest = trimmed.slice(schemeMatch[0].length);
  }

  if (rest.length === 0) {
    return null;
  }
  // No path, query, fragment, userinfo or bracketed literals.
  if (/[/?#@\\[\]]/.test(rest)) {
    return null;
  }

  const colons = (rest.match(/:/g) ?? []).length;
  if (colons > 1) {
    return null;
  }

  let host = rest.toLowerCase();
  let port = DEFAULT_JOIN_PORT;
  if (colons === 1) {
    const splitAt = rest.lastIndexOf(':');
    host = rest.slice(0, splitAt).toLowerCase();
    const portText = rest.slice(splitAt + 1);
    if (!/^[0-9]{1,5}$/.test(portText)) {
      return null;
    }
    port = Number.parseInt(portText, 10);
    if (port < 1 || port > 65535) {
      return null;
    }
  }

  if (!isValidHost(host)) {
    return null;
  }
  if (IPV4.test(host)) {
    host = host
      .split('.')
      .map((label) => String(Number.parseInt(label, 10)))
      .join('.');
  }
  return `${scheme}://${host}:${port}`;
}
