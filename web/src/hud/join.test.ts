import { describe, expect, it } from 'vitest';
import { DEFAULT_JOIN_PORT, parseJoinAddress } from './join';

describe('parseJoinAddress', () => {
  it('accepts host:port without a scheme', () => {
    expect(parseJoinAddress('192.168.1.23:7879')).toBe('http://192.168.1.23:7879');
  });

  it('keeps an explicit http or https scheme', () => {
    expect(parseJoinAddress('http://192.168.1.23:7879')).toBe('http://192.168.1.23:7879');
    expect(parseJoinAddress('https://games.example.com:8443')).toBe('https://games.example.com:8443');
  });

  it('defaults the port for a bare address', () => {
    expect(parseJoinAddress('192.168.1.23')).toBe(`http://192.168.1.23:${DEFAULT_JOIN_PORT}`);
    expect(parseJoinAddress('https://192.168.1.23')).toBe(`https://192.168.1.23:${DEFAULT_JOIN_PORT}`);
  });

  it('trims surrounding whitespace and rejects inner whitespace', () => {
    expect(parseJoinAddress('  192.168.1.23:7879  ')).toBe('http://192.168.1.23:7879');
    expect(parseJoinAddress('http://192.168.1.23: 7879')).toBeNull();
    expect(parseJoinAddress('hello world')).toBeNull();
  });

  it('rejects blank and whitespace-only input', () => {
    expect(parseJoinAddress('')).toBeNull();
    expect(parseJoinAddress('   ')).toBeNull();
  });

  it('normalises case and ip leading zeros', () => {
    expect(parseJoinAddress('HTTP://LOCALHOST:7879')).toBe('http://localhost:7879');
    expect(parseJoinAddress('http://01.2.3.4:7879')).toBe('http://1.2.3.4:7879');
  });

  it('accepts the port boundaries', () => {
    expect(parseJoinAddress('host.com:1')).toBe('http://host.com:1');
    expect(parseJoinAddress('host.com:65535')).toBe('http://host.com:65535');
  });

  it('rejects non-http(s) schemes', () => {
    expect(parseJoinAddress('ftp://192.168.1.23:7879')).toBeNull();
    expect(parseJoinAddress('ws://192.168.1.23:7879')).toBeNull();
    expect(parseJoinAddress('file:///tmp/x')).toBeNull();
  });

  it('rejects a missing host', () => {
    expect(parseJoinAddress('http://')).toBeNull();
  });

  it('rejects a missing, invalid or out-of-range port after the colon', () => {
    expect(parseJoinAddress('http://192.168.1.23:')).toBeNull();
    expect(parseJoinAddress('http://192.168.1.23:abc')).toBeNull();
    expect(parseJoinAddress('http://192.168.1.23:0')).toBeNull();
    expect(parseJoinAddress('http://192.168.1.23:65536')).toBeNull();
  });

  it('rejects a path, query, fragment or userinfo', () => {
    expect(parseJoinAddress('http://host.com:7879/play/')).toBeNull();
    expect(parseJoinAddress('http://host.com:7879/?x=1')).toBeNull();
    expect(parseJoinAddress('http://host.com:7879#top')).toBeNull();
    expect(parseJoinAddress('http://user:pass@host.com:7879')).toBeNull();
  });

  it('rejects outright junk', () => {
    expect(parseJoinAddress('not a url')).toBeNull();
    expect(parseJoinAddress(':::')).toBeNull();
    expect(parseJoinAddress('[::1]:7879')).toBeNull();
    expect(parseJoinAddress('999.999.999.999:7879')).toBeNull();
    expect(parseJoinAddress('-lead.com:7879')).toBeNull();
    expect(parseJoinAddress('trail-.com:7879')).toBeNull();
    expect(parseJoinAddress('double..dot.com:7879')).toBeNull();
  });
});
