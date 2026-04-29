import { test } from 'node:test';
import * as assert from 'node:assert/strict';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { scanRoutes } from '../route-scanner';

interface FixtureFile {
  rel: string;
  contents?: string;
}

function makeFixture(files: FixtureFile[]): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'sm-nuxt-'));
  fs.writeFileSync(path.join(dir, 'package.json'), '{}');
  fs.writeFileSync(path.join(dir, 'nuxt.config.ts'), 'export default {}');
  for (const f of files) {
    const target = path.join(dir, f.rel);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, f.contents ?? '// stub');
  }
  return dir;
}

test('returns empty when no server dir', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'sm-nuxt-'));
  const routes = await scanRoutes(dir);
  assert.deepEqual(routes, []);
});

test('discovers basic api route under /api prefix', async () => {
  const dir = makeFixture([{ rel: 'server/api/hello.ts' }]);
  const routes = await scanRoutes(dir);
  assert.equal(routes.length, 1);
  assert.equal(routes[0].path, '/api/hello');
  assert.equal(routes[0].type, 'api');
  assert.equal(routes[0].isDynamic, false);
});

test('server/routes is not /api prefixed', async () => {
  const dir = makeFixture([{ rel: 'server/routes/health.ts' }]);
  const routes = await scanRoutes(dir);
  assert.equal(routes.length, 1);
  assert.equal(routes[0].path, '/health');
  assert.equal(routes[0].type, 'route');
});

test('index file resolves to parent prefix', async () => {
  const dir = makeFixture([
    { rel: 'server/api/index.ts' },
    { rel: 'server/api/users/index.ts' },
  ]);
  const routes = await scanRoutes(dir);
  const paths = routes.map((r) => r.path).sort();
  assert.deepEqual(paths, ['/api', '/api/users']);
});

test('dynamic segment [id] becomes :id', async () => {
  const dir = makeFixture([{ rel: 'server/api/users/[id].ts' }]);
  const routes = await scanRoutes(dir);
  assert.equal(routes.length, 1);
  assert.equal(routes[0].path, '/api/users/:id');
  assert.equal(routes[0].isDynamic, true);
});

test('catch-all [...slug] becomes *', async () => {
  const dir = makeFixture([{ rel: 'server/api/posts/[...slug].ts' }]);
  const routes = await scanRoutes(dir);
  assert.equal(routes.length, 1);
  assert.equal(routes[0].path, '/api/posts/*');
});

test('dynamic + catch-all in same tree', async () => {
  const dir = makeFixture([
    { rel: 'server/api/users/[id].ts' },
    { rel: 'server/api/users/[id]/posts/[...rest].ts' },
  ]);
  const routes = await scanRoutes(dir);
  const paths = routes.map((r) => r.path).sort();
  assert.deepEqual(paths, ['/api/users/:id', '/api/users/:id/posts/*']);
});

test('route group (group) is stripped from URL', async () => {
  const dir = makeFixture([
    { rel: 'server/api/(internal)/admin.ts' },
  ]);
  const routes = await scanRoutes(dir);
  assert.equal(routes.length, 1);
  assert.equal(routes[0].path, '/api/admin');
});

test('method suffixes infer methods and merge into one entry', async () => {
  const dir = makeFixture([
    { rel: 'server/api/foo.get.ts' },
    { rel: 'server/api/foo.post.ts' },
  ]);
  const routes = await scanRoutes(dir);
  assert.equal(routes.length, 1);
  assert.equal(routes[0].path, '/api/foo');
  assert.deepEqual(routes[0].methods?.sort(), ['GET', 'POST']);
});

test('plain file alongside method-suffixed file collapses methods to all', async () => {
  const dir = makeFixture([
    { rel: 'server/api/bar.ts' },
    { rel: 'server/api/bar.get.ts' },
  ]);
  const routes = await scanRoutes(dir);
  assert.equal(routes.length, 1);
  assert.equal(routes[0].path, '/api/bar');
  assert.equal(routes[0].methods, undefined);
});

test('underscore and dot prefix files are skipped', async () => {
  const dir = makeFixture([
    { rel: 'server/api/_internal.ts' },
    { rel: 'server/api/.hidden.ts' },
    { rel: 'server/api/visible.ts' },
  ]);
  const routes = await scanRoutes(dir);
  assert.equal(routes.length, 1);
  assert.equal(routes[0].path, '/api/visible');
});

test('underscore-prefixed directory is skipped', async () => {
  const dir = makeFixture([
    { rel: 'server/api/_private/secret.ts' },
    { rel: 'server/api/public.ts' },
  ]);
  const routes = await scanRoutes(dir);
  assert.equal(routes.length, 1);
  assert.equal(routes[0].path, '/api/public');
});

test('non-route extensions are ignored', async () => {
  const dir = makeFixture([
    { rel: 'server/api/note.md' },
    { rel: 'server/api/data.json' },
    { rel: 'server/api/handler.ts' },
  ]);
  const routes = await scanRoutes(dir);
  assert.equal(routes.length, 1);
  assert.equal(routes[0].path, '/api/handler');
});

test('middleware is surfaced but tagged middleware', async () => {
  const dir = makeFixture([
    { rel: 'server/middleware/auth.ts' },
    { rel: 'server/api/me.ts' },
  ]);
  const routes = await scanRoutes(dir);
  const middleware = routes.filter((r) => r.type === 'middleware');
  const api = routes.filter((r) => r.type === 'api');
  assert.equal(middleware.length, 1);
  assert.equal(middleware[0].filePath.endsWith('auth.ts'), true);
  assert.equal(api.length, 1);
});

test('mts and mjs extensions are recognized', async () => {
  const dir = makeFixture([
    { rel: 'server/api/a.mts' },
    { rel: 'server/api/b.mjs' },
  ]);
  const routes = await scanRoutes(dir);
  const paths = routes.map((r) => r.path).sort();
  assert.deepEqual(paths, ['/api/a', '/api/b']);
});
