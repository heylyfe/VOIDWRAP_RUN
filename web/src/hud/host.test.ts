import { describe, expect, it } from 'vitest';
import { hostUrl, parseHostResponse } from './host';

const valid = {
  hosting: true,
  port: 7879,
  lan: ['192.168.1.23'],
  public: '203.0.113.5',
  upnp: 'open',
  message: 'Port opened',
};

describe('parseHostResponse', () => {
  it('accepts a valid response unchanged', () => {
    expect(parseHostResponse(valid)).toEqual(valid);
  });

  it('accepts a null public address and empty lan', () => {
    const parsed = parseHostResponse({ ...valid, lan: [], public: null, upnp: 'off', hosting: false });
    expect(parsed?.public).toBeNull();
    expect(parsed?.lan).toEqual([]);
    expect(parsed?.hosting).toBe(false);
  });

  it('accepts every upnp state', () => {
    for (const upnp of ['off', 'trying', 'open', 'failed']) {
      expect(parseHostResponse({ ...valid, upnp })).not.toBeNull();
    }
  });

  it('rejects non-objects and missing or mistyped fields', () => {
    expect(parseHostResponse(null)).toBeNull();
    expect(parseHostResponse('nope')).toBeNull();
    expect(parseHostResponse({ ...valid, hosting: 'yes' })).toBeNull();
    expect(parseHostResponse({ ...valid, port: '7879' })).toBeNull();
    expect(parseHostResponse({ ...valid, port: 0 })).toBeNull();
    expect(parseHostResponse({ ...valid, port: 70000 })).toBeNull();
    expect(parseHostResponse({ port: 7879 })).toBeNull();
  });

  it('rejects bad lan values, bad public values and unknown upnp states', () => {
    expect(parseHostResponse({ ...valid, lan: '192.168.1.23' })).toBeNull();
    expect(parseHostResponse({ ...valid, lan: ['ok', 5] })).toBeNull();
    expect(parseHostResponse({ ...valid, public: 123 })).toBeNull();
    expect(parseHostResponse({ ...valid, upnp: 'pending' })).toBeNull();
    expect(parseHostResponse({ ...valid, message: undefined })).toBeNull();
  });
});

describe('hostUrl', () => {
  it('builds an http url from a bare address and the port', () => {
    expect(hostUrl('192.168.1.23', 7879)).toBe('http://192.168.1.23:7879');
    expect(hostUrl('games.example.com', 8000)).toBe('http://games.example.com:8000');
  });

  it('trims whitespace', () => {
    expect(hostUrl('  192.168.1.23  ', 7879)).toBe('http://192.168.1.23:7879');
  });

  it('passes an already absolute http(s) url through, minus trailing slashes', () => {
    expect(hostUrl('http://192.168.1.23:7879/', 7879)).toBe('http://192.168.1.23:7879');
    expect(hostUrl('https://games.example.com:9000//', 7879)).toBe('https://games.example.com:9000');
  });
});
