import { test } from 'node:test';
import * as assert from 'node:assert/strict';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { scanRoutes, dirToSegment } from '../route-scanner';

function mkProject(files: Record<string, string>): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'sk-scanner-'));
  for (const [rel, content] of Object.entries(files)) {
    const full = path.join(dir, rel);
    fs.mkdirSync(path.dirname(full), { recursive: true });
    fs.writeFileSync(full, content);
  }
  return dir;
}

test('dirToSegment maps SvelteKit naming conventions', () => {
  assert.equal(dirToSegment('users'), 'users');
  assert.equal(dirToSegment('[id]'), ':id');
  assert.equal(dirToSegment('[id=integer]'), ':id');
  assert.equal(dirToSegment('[[optional]]'), ':optional');
  assert.equal(dirToSegment('[[optional=slug]]'), ':optional');
  assert.equal(dirToSegment('[...slug]'), '*');
  assert.equal(dirToSegment('(marketing)'), '');
  assert.equal(dirToSegment('(group=ignored)'), '');
});

test('scanRoutes finds nested API routes with params and catch-alls', async () => {
  const dir = mkProject({
    'src/routes/+server.ts': 'export const GET = () => {};',
    'src/routes/api/users/+server.ts': 'export const GET = () => {}; export const POST = () => {};',
    'src/routes/api/users/[id]/+server.ts': 'export const PUT = () => {};',
    'src/routes/blog/[...slug]/+server.ts': 'export const GET = () => {};',
    'src/routes/(marketing)/about/+page.svelte': '<h1>about</h1>',
    'src/routes/[id=integer]/+server.ts': 'export const GET = () => {};',
  });

  const routes = await scanRoutes(dir);
  const byPath = new Map(routes.map((r) => [`${r.type}::${r.path}`, r]));

  assert.ok(byPath.has('api::/'), 'root +server.ts at /');
  assert.ok(byPath.has('api::/api/users'), 'nested users endpoint');
  assert.ok(byPath.has('api::/api/users/:id'), 'param users/:id');
  assert.ok(byPath.has('api::/blog/*'), 'catch-all blog/*');
  assert.ok(byPath.has('page::/about'), 'route group stripped');
  assert.ok(byPath.has('api::/:id'), 'param matcher stripped');
});

test('scanRoutes skips files prefixed with _ or .', async () => {
  const dir = mkProject({
    'src/routes/api/+server.ts': 'export const GET = () => {};',
    'src/routes/_internal/+server.ts': 'export const GET = () => {};',
    'src/routes/.hidden/+server.ts': 'export const GET = () => {};',
  });

  const routes = await scanRoutes(dir);
  assert.equal(routes.length, 1);
  assert.equal(routes[0].path, '/api');
});

test('scanRoutes returns empty when src/routes does not exist', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'sk-scanner-empty-'));
  const routes = await scanRoutes(dir);
  assert.deepEqual(routes, []);
});

test('scanRoutes detects +page.server.ts and +layout.svelte', async () => {
  const dir = mkProject({
    'src/routes/+layout.svelte': '<slot />',
    'src/routes/login/+page.server.ts': 'export const actions = {}; export const load = () => {};',
    'src/routes/login/+page.svelte': '<form />',
  });

  const routes = await scanRoutes(dir);
  const types = routes.map((r) => `${r.type}::${r.path}`).sort();
  assert.deepEqual(types.sort(), ['layout::/', 'page-server::/login', 'page::/login'].sort());
});

test('scanRoutes ignores non-+ prefixed files', async () => {
  const dir = mkProject({
    'src/routes/api/helper.ts': 'export const x = 1;',
    'src/routes/api/+server.ts': 'export const GET = () => {};',
  });

  const routes = await scanRoutes(dir);
  assert.equal(routes.length, 1);
  assert.equal(routes[0].type, 'api');
});

test('scanRoutes accepts .js endpoints alongside .ts', async () => {
  const dir = mkProject({
    'src/routes/api/+server.js': 'export const GET = () => {};',
  });

  const routes = await scanRoutes(dir);
  assert.equal(routes.length, 1);
  assert.equal(routes[0].path, '/api');
});
