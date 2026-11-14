// Pure helpers for the hosting panel: the shape the game program answers on
// GET/POST /api/host, and how to show an address. Unit-tested.
export type UpnpState = 'off' | 'trying' | 'open' | 'failed';

export interface HostStatus {
  hosting: boolean;
  port: number;
  lan: string[];
  public: string | null;
  upnp: UpnpState;
  message: string;
}

/** Validate an unknown value as a HostStatus; null when anything is off. */
export function parseHostResponse(value: unknown): HostStatus | null {
  if (typeof value !== 'object' || value === null) {
    return null;
  }
  const record = value as Record<string, unknown>;
  const { hosting, port, lan, public: publicAddress, upnp, message } = record;

  if (typeof hosting !== 'boolean') {
    return null;
  }
  if (typeof port !== 'number' || !Number.isFinite(port) || port < 1 || port > 65535) {
    return null;
  }
  if (!Array.isArray(lan)) {
    return null;
  }
  const lanList: string[] = [];
  for (const item of lan) {
    if (typeof item !== 'string') {
      return null;
    }
    lanList.push(item);
  }
  if (!(typeof publicAddress === 'string' || publicAddress === null)) {
    return null;
  }
  if (upnp !== 'off' && upnp !== 'trying' && upnp !== 'open' && upnp !== 'failed') {
    return null;
  }
  if (typeof message !== 'string') {
    return null;
  }
  return { hosting, port, lan: lanList, public: publicAddress, upnp, message };
}

/**
 * Show an address the way a friend should type it. Bare addresses get the
 * http scheme and the given port; an already absolute http(s) URL is used
 * as-is. Trailing slashes are stripped.
 */
export function hostUrl(address: string, port: number): string {
  const trimmed = address.trim().replace(/\/+$/, '');
  if (/^https?:\/\//i.test(trimmed)) {
    return trimmed;
  }
  return `http://${trimmed}:${port}`;
}
