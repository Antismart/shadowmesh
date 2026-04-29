import { test } from 'node:test';
import * as assert from 'node:assert/strict';
import {
  generateManifest,
  routeToHandlerName,
} from '../manifest-generator';
import type { ScannedRoute } from '../route-scanner';

function api(routePath: string, methods?: ScannedRoute['methods']): ScannedRoute {
  return {
    path: routePath,
    type: 'api',
    filePath: '<test>',
    isDynamic: routePath.includes(':') || routePath.includes('*'),
    methods,
  };
}

test('handler name strips slashes, colons and wildcards', () => {
  assert.equal(routeToHandlerName('/api/users/:id'), 'api-users-id');
  assert.equal(routeToHandlerName('/api/posts/*'), 'api-posts');
  assert.equal(routeToHandlerName('/health'), 'health');
  assert.equal(routeToHandlerName('/'), 'index');
});

test('handler name never returns empty or .. traversal', () => {
  assert.equal(routeToHandlerName(''), 'index');
  assert.equal(routeToHandlerName('////'), 'index');
});

test('handler is a bare filename (no slashes, no ..)', () => {
  const m = generateManifest([api('/api/users/:id')]);
  assert.equal(m.routes.length, 1);
  const handler = m.routes[0].handler;
  assert.ok(!handler.includes('/'));
  assert.ok(!handler.includes('\\'));
  assert.ok(!handler.includes('..'));
  assert.ok(handler.length > 0);
});

test('default methods when none specified', () => {
  const m = generateManifest([api('/api/foo')]);
  assert.deepEqual(m.routes[0].methods, [
    'GET',
    'POST',
    'PUT',
    'DELETE',
    'PATCH',
  ]);
});

test('preserves declared methods', () => {
  const m = generateManifest([api('/api/foo', ['GET', 'POST'])]);
  assert.deepEqual(m.routes[0].methods, ['GET', 'POST']);
});

test('always emits version 1 and static [/**] by default', () => {
  const m = generateManifest([api('/api/x')]);
  assert.equal(m.version, 1);
  assert.deepEqual(m.static, ['/**']);
});

test('staticPatterns option overrides default', () => {
  const m = generateManifest([api('/api/x')], {
    staticPatterns: ['/assets/**', '/img/**'],
  });
  assert.deepEqual(m.static, ['/assets/**', '/img/**']);
});

test('empty input yields routes: [] with no synthetic catch-all', () => {
  const m = generateManifest([]);
  assert.deepEqual(m.routes, []);
  assert.deepEqual(m.static, ['/**']);
});

test('edgeRoutes filter excludes non-matching routes', () => {
  const m = generateManifest(
    [api('/api/foo'), api('/api/bar'), api('/api/admin/x')],
    { edgeRoutes: ['/api/admin/*'] }
  );
  assert.equal(m.routes.length, 1);
  assert.equal(m.routes[0].path, '/api/admin/x');
});

test('capabilities passed through when non-empty', () => {
  const m = generateManifest([api('/api/x')], {
    capabilities: { 'api-x.wasm': ['net:connect'] },
  });
  assert.deepEqual(m.capabilities, { 'api-x.wasm': ['net:connect'] });
});

test('capabilities omitted when empty/undefined', () => {
  const m = generateManifest([api('/api/x')]);
  assert.equal(m.capabilities, undefined);
});

test('duplicate handler names are de-duplicated', () => {
  const m = generateManifest([api('/api/users'), api('/api-users')]);
  const handlers = m.routes.map((r) => r.handler);
  assert.equal(new Set(handlers).size, handlers.length);
});

test('catch-all path stays * only as last segment', () => {
  const m = generateManifest([api('/api/posts/*')]);
  assert.equal(m.routes[0].path, '/api/posts/*');
});

test('route entries are JSON-serializable and conform to schema', () => {
  const m = generateManifest([
    api('/api/users/:id', ['GET']),
    api('/api/posts/*', ['POST']),
  ]);
  const json = JSON.parse(JSON.stringify(m));
  assert.equal(json.version, 1);
  assert.ok(Array.isArray(json.routes));
  for (const r of json.routes) {
    assert.ok(typeof r.path === 'string' && r.path.startsWith('/'));
    assert.ok(typeof r.handler === 'string' && r.handler.length > 0);
    assert.ok(!r.handler.includes('/') && !r.handler.includes('..'));
    for (const method of r.methods) {
      assert.ok(
        ['GET', 'POST', 'PUT', 'DELETE', 'PATCH', 'HEAD', 'OPTIONS'].includes(
          method
        )
      );
    }
  }
});
