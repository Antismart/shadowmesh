import { test } from 'node:test';
import * as assert from 'node:assert/strict';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { generateManifest, routeToHandlerStem } from '../manifest-generator';
import type { ScannedRoute } from '../route-scanner';

function tmpFile(contents: string): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'remix-mf-'));
  const f = path.join(dir, 'route.tsx');
  fs.writeFileSync(f, contents);
  return f;
}

test('routeToHandlerStem: paths to safe stems', () => {
  assert.equal(routeToHandlerStem('/'), 'index');
  assert.equal(routeToHandlerStem('/about'), 'about');
  assert.equal(routeToHandlerStem('/users/:id'), 'users-id');
  assert.equal(routeToHandlerStem('/api/users/:id'), 'api-users-id');
  assert.equal(routeToHandlerStem('/blog/*'), 'blog-splat');
});

test('routeToHandlerStem: no slashes, dots, or empty', () => {
  for (const p of ['/', '/a', '/a/:b', '/a/*', '/api/:x/:y']) {
    const stem = routeToHandlerStem(p);
    assert.ok(!stem.includes('/'), `${stem} contains /`);
    assert.ok(!stem.includes('..'), `${stem} contains ..`);
    assert.ok(stem.length > 0, `${stem} is empty`);
  }
});

test('generateManifest: basic structure conforms to gateway schema', async () => {
  const loaderFile = tmpFile('export const loader = () => null;');
  const actionFile = tmpFile('export const action = () => null;');
  const uiFile = tmpFile('export default function Page() {}');

  const routes: ScannedRoute[] = [
    { path: '/', filePath: uiFile, isIndex: true, isSplat: false, isDynamic: false },
    { path: '/users/:id', filePath: loaderFile, isIndex: false, isSplat: false, isDynamic: true },
    { path: '/api/post', filePath: actionFile, isIndex: false, isSplat: false, isDynamic: false },
  ];

  const m = await generateManifest(routes);
  assert.equal(m.version, 1);
  assert.deepEqual(m.static, ['/**']);
  assert.equal(m.routes.length, 3);
  assert.deepEqual(
    m.routes.find((r) => r.path === '/')?.methods,
    ['GET']
  );
  assert.deepEqual(
    m.routes.find((r) => r.path === '/users/:id')?.methods,
    ['GET']
  );
  assert.deepEqual(
    m.routes.find((r) => r.path === '/api/post')?.methods.sort(),
    ['DELETE', 'PATCH', 'POST', 'PUT']
  );

  for (const r of m.routes) {
    assert.ok(!r.handler.includes('/'));
    assert.ok(!r.handler.includes('\\'));
    assert.ok(!r.handler.includes('..'));
    assert.ok(r.handler.length > 0);
    assert.ok(r.handler.endsWith('.wasm'));
  }
});

test('generateManifest: handler collisions disambiguated', async () => {
  const f = tmpFile('export default () => null;');
  const routes: ScannedRoute[] = [
    { path: '/about', filePath: f, isIndex: false, isSplat: false, isDynamic: false },
    { path: '/about', filePath: f, isIndex: false, isSplat: false, isDynamic: false },
  ];
  const m = await generateManifest(routes);
  assert.equal(m.routes[0].handler, 'about.wasm');
  assert.equal(m.routes[1].handler, 'about-2.wasm');
});

test('generateManifest: empty routes is valid', async () => {
  const m = await generateManifest([]);
  assert.equal(m.version, 1);
  assert.deepEqual(m.routes, []);
  assert.deepEqual(m.static, ['/**']);
});

test('generateManifest: capabilities passthrough', async () => {
  const m = await generateManifest([], {
    capabilities: { 'api.wasm': ['net:connect'] },
  });
  assert.deepEqual(m.capabilities, { 'api.wasm': ['net:connect'] });
});

test('generateManifest: methods are valid HTTP verbs', async () => {
  const valid = new Set(['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS']);
  const f = tmpFile('export const loader = ()=>null; export const action = ()=>null;');
  const m = await generateManifest([
    { path: '/x', filePath: f, isIndex: false, isSplat: false, isDynamic: false },
  ]);
  for (const method of m.routes[0].methods) {
    assert.ok(valid.has(method));
  }
});
