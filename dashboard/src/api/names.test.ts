import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { names } from './names';

// Mock global fetch
const mockFetch = vi.fn();

beforeEach(() => {
  mockFetch.mockClear();
  vi.stubGlobal('fetch', mockFetch);
});

afterEach(() => {
  vi.restoreAllMocks();
});

function mockJsonResponse(data: unknown, status = 200) {
  mockFetch.mockResolvedValueOnce({
    ok: status >= 200 && status < 300,
    status,
    statusText: 'OK',
    json: () => Promise.resolve(data),
  });
}

describe('names API', () => {
  it('list() calls GET /api/names', async () => {
    const payload = [{ name: 'my-site', name_hash: 'abc123' }];
    mockJsonResponse(payload);

    const result = await names.list();

    expect(mockFetch).toHaveBeenCalledOnce();
    const [url, options] = mockFetch.mock.calls[0];
    expect(url).toBe('/api/names');
    expect(options.method).toBeUndefined(); // GET by default
    expect(result).toEqual(payload);
  });

  it('resolve() calls GET /api/names/:name', async () => {
    const payload = { found: true, record: { name: 'test' } };
    mockJsonResponse(payload);

    const result = await names.resolve('test');

    const [url] = mockFetch.mock.calls[0];
    expect(url).toBe('/api/names/test');
    expect(result).toEqual(payload);
  });

  it('resolve() encodes special characters in the name', async () => {
    mockJsonResponse({ found: false });

    await names.resolve('my site/foo');

    const [url] = mockFetch.mock.calls[0];
    expect(url).toBe('/api/names/my%20site%2Ffoo');
  });

  it('assign() calls POST /api/names/:name/assign with cid body', async () => {
    const payload = { success: true, name: 'app', name_hash: 'h', cid: 'Qm123' };
    mockJsonResponse(payload);

    const result = await names.assign('app', 'Qm123');

    const [url, options] = mockFetch.mock.calls[0];
    expect(url).toBe('/api/names/app/assign');
    expect(options.method).toBe('POST');
    expect(options.headers).toEqual(
      expect.objectContaining({ 'Content-Type': 'application/json' }),
    );
    expect(JSON.parse(options.body)).toEqual({ cid: 'Qm123' });
    expect(result).toEqual(payload);
  });

  it('remove() calls DELETE /api/names/:name', async () => {
    mockJsonResponse({ success: true });

    const result = await names.remove('old-site');

    const [url, options] = mockFetch.mock.calls[0];
    expect(url).toBe('/api/names/old-site');
    expect(options.method).toBe('DELETE');
    expect(result).toEqual({ success: true });
  });
});
