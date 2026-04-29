import { test } from 'node:test';
import * as assert from 'node:assert/strict';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { generateManifest, routeToHandlerName } from '../manifest-generator';
import type { ScannedRoute } from '../route-scanner';

function tmpFile(content: string, name = 'plus-server.ts'): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'sk-manifest-'));
  const p = path.join(dir, name);
  fs.writeFileSync(p, content);
  return p;
}

test('routeToHandlerName produces safe bare filenames', () => {
  assert.equal(routeToHandlerName('/'), 'index');
  assert.equal(routeToHandlerName('/api/users'), 'api-users');
  assert.equal(routeToHandlerName('/api/users/:id'), 'api-users-id');
  assert.equal(routeToHandlerName('/blog/*'), 'blog');
  assert.equal(routeToHandlerName('/'), 'index');
  assert.equal(routeToHandlerName('/.././etc'), 'etc');
});

test('routeToHandlerName never contains separators or traversal', () => {
  for (const p of ['/api/users', '/a/b/c/:d/*', '/.././x', '/']) {
    const h = routeToHandlerName(p);
    assert.ok(!h.includes('/'), `no slash in ${h}`);
    assert.ok(!h.includes('\\'), `no backslash in ${h}`);
    assert.ok(!h.includes('..'), `no traversal in ${h}`);
    assert.notEqual(h, '');
  }
});

test('generateManifest defaults methods to ["GET"] when none detected', async () => {
  const file = tmpFile('export const helper = 1;');
  const routes: ScannedRoute[] = [
    { path: '/api', type: 'api', filePath: file, isDynamic: false },
  ];
  const m = await generateManifest(routes);
  assert.equal(m.version, 1);
  assert.equal(m.routes.length, 1);
  assert.deepEqual(m.routes[0].methods, ['GET']);
  assert.deepEqual(m.static, ['/**']);
});

test('generateManifest reads detected methods from +server.ts source', async () => {
  const file = tmpFile(`
    export const GET = () => {};
    export const POST = () => {};
    export const DELETE = () => {};
  `);
  const routes: ScannedRoute[] = [
    { path: '/api/users', type: 'api', filePath: file, isDynamic: false },
  ];
  const m = await generateManifest(routes);
  assert.deepEqual(m.routes[0].methods, ['GET', 'POST', 'DELETE']);
});

test('generateManifest skips page and layout entries', async () => {
  const file = tmpFile('<h1>x</h1>', 'plus-page.svelte');
  const routes: ScannedRoute[] = [
    { path: '/', type: 'page', filePath: file, isDynamic: false },
    { path: '/', type: 'layout', filePath: file, isDynamic: false },
  ];
  const m = await generateManifest(routes);
  assert.equal(m.routes.length, 0);
});

test('generateManifest treats page-server with load+actions as GET+POST', async () => {
  const file = tmpFile(`
    export const load = () => ({});
    export const actions = { default: () => {} };
  `);
  const routes: ScannedRoute[] = [
    { path: '/login', type: 'page-server', filePath: file, isDynamic: false },
  ];
  const m = await generateManifest(routes);
  assert.deepEqual(m.routes[0].methods, ['GET', 'POST']);
});

test('generateManifest emits handler conforming to gateway parser', async () => {
  const file = tmpFile('export const GET = () => {};');
  const routes: ScannedRoute[] = [
    { path: '/api/users/:id', type: 'api', filePath: file, isDynamic: true },
  ];
  const m = await generateManifest(routes);
  const handler = m.routes[0].handler;
  assert.ok(!handler.includes('/'));
  assert.ok(!handler.includes('\\'));
  assert.ok(!handler.includes('..'));
  assert.notEqual(handler, '');
  assert.equal(handler, 'api-users-id.wasm');
});

test('generateManifest respects staticPatterns and capabilities options', async () => {
  const file = tmpFile('export const GET = () => {};');
  const routes: ScannedRoute[] = [
    { path: '/api', type: 'api', filePath: file, isDynamic: false },
  ];
  const m = await generateManifest(routes, {
    staticPatterns: ['/static/**'],
    capabilities: { 'api.wasm': ['net:connect'] },
  });
  assert.deepEqual(m.static, ['/static/**']);
  assert.deepEqual(m.capabilities, { 'api.wasm': ['net:connect'] });
});

test('generateManifest dedupes duplicate path+handler entries', async () => {
  const file = tmpFile('export const GET = () => {};');
  const routes: ScannedRoute[] = [
    { path: '/api', type: 'api', filePath: file, isDynamic: false },
    { path: '/api', type: 'api', filePath: file, isDynamic: false },
  ];
  const m = await generateManifest(routes);
  assert.equal(m.routes.length, 1);
});

test('generateManifest emits empty routes array for an empty project', async () => {
  const m = await generateManifest([]);
  assert.deepEqual(m.routes, []);
  assert.deepEqual(m.static, ['/**']);
});

test('generateManifest skips capabilities key when empty', async () => {
  const m = await generateManifest([], { capabilities: {} });
  assert.equal(m.capabilities, undefined);
});

test('generateManifest accepts methodOverrides keyed by filePath', async () => {
  const routes: ScannedRoute[] = [
    { path: '/api', type: 'api', filePath: '/virtual/+server.ts', isDynamic: false },
  ];
  const m = await generateManifest(routes, {
    methodOverrides: { '/virtual/+server.ts': ['PATCH'] },
  });
  assert.deepEqual(m.routes[0].methods, ['PATCH']);
});
