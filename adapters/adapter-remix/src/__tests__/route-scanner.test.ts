import { test } from 'node:test';
import * as assert from 'node:assert/strict';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { scanRoutes, v2FilenameToPath, detectConvention } from '../route-scanner';

function makeTmp(prefix: string): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), prefix));
}

function write(dir: string, rel: string, contents = ''): string {
  const full = path.join(dir, rel);
  fs.mkdirSync(path.dirname(full), { recursive: true });
  fs.writeFileSync(full, contents);
  return full;
}

test('v2FilenameToPath: basic conversions', () => {
  assert.equal(v2FilenameToPath('_index'), '/');
  assert.equal(v2FilenameToPath('about'), '/about');
  assert.equal(v2FilenameToPath('users.$id'), '/users/:id');
  assert.equal(v2FilenameToPath('blog.$'), '/blog/*');
  assert.equal(v2FilenameToPath('users_.profile'), '/users/profile');
  assert.equal(v2FilenameToPath('_marketing.about'), '/about');
  assert.equal(v2FilenameToPath('($lang).about'), '/about');
  assert.equal(v2FilenameToPath('api.users.$id'), '/api/users/:id');
});

test('v2FilenameToPath: escaped literal segments', () => {
  assert.equal(v2FilenameToPath('sitemap[.]xml'), '/sitemap.xml');
});

test('v2FilenameToPath: splat must be last segment', () => {
  // `$.foo` would imply splat in non-final position — invalid.
  assert.equal(v2FilenameToPath('$.foo'), null);
});

test('scanRoutes: v2 flat routes', async () => {
  const dir = makeTmp('remix-v2-');
  write(dir, 'app/routes/_index.tsx', 'export const loader = () => null;');
  write(dir, 'app/routes/about.tsx', '');
  write(dir, 'app/routes/users.$id.tsx', 'export const loader = () => null;');
  write(dir, 'app/routes/blog.$.tsx', '');
  write(dir, 'app/routes/_marketing.about.tsx', '');
  write(dir, 'app/routes/users_.profile.tsx', '');
  write(dir, 'app/routes/_marketing.tsx', ''); // pathless layout — skip
  write(dir, 'app/routes/.dotfile.tsx', '');   // skip

  const routes = await scanRoutes(dir, { convention: 'v2-flat' });
  const paths = routes.map((r) => r.path).sort();
  // _marketing.about and about both produce /about — both files yielded.
  assert.deepEqual(paths, ['/', '/about', '/about', '/blog/*', '/users/profile', '/users/:id'].sort());

  const splat = routes.find((r) => r.path === '/blog/*');
  assert.ok(splat?.isSplat);

  const idx = routes.find((r) => r.path === '/');
  assert.ok(idx?.isIndex);
});

test('scanRoutes: v1 nested routes', async () => {
  const dir = makeTmp('remix-v1-');
  write(dir, 'app/routes/index.tsx', '');
  write(dir, 'app/routes/about.tsx', '');
  write(dir, 'app/routes/users/$id.tsx', 'export const loader = () => null;');
  write(dir, 'app/routes/users/index.tsx', '');
  write(dir, 'app/routes/blog/$.tsx', '');
  write(dir, 'app/routes/__marketing/promo.tsx', ''); // pathless layout dir → /promo

  const routes = await scanRoutes(dir, { convention: 'v1-nested' });
  const paths = routes.map((r) => r.path).sort();
  assert.deepEqual(
    paths,
    ['/', '/about', '/blog/*', '/promo', '/users', '/users/:id'].sort()
  );

  assert.ok(routes.find((r) => r.path === '/blog/*')?.isSplat);
  assert.ok(routes.find((r) => r.path === '/users/:id')?.isDynamic);
});

test('scanRoutes: empty when no app/routes/', async () => {
  const dir = makeTmp('remix-empty-');
  const routes = await scanRoutes(dir);
  assert.deepEqual(routes, []);
});

test('detectConvention: nested dirs → v1', async () => {
  const dir = makeTmp('remix-detect-v1-');
  write(dir, 'app/routes/users/index.tsx', '');
  const c = await detectConvention(dir, path.join(dir, 'app/routes'));
  assert.equal(c, 'v1-nested');
});

test('detectConvention: flat files → v2', async () => {
  const dir = makeTmp('remix-detect-v2-');
  write(dir, 'app/routes/_index.tsx', '');
  write(dir, 'app/routes/about.tsx', '');
  const c = await detectConvention(dir, path.join(dir, 'app/routes'));
  assert.equal(c, 'v2-flat');
});

test('detectConvention: vite.config referencing @remix-run/dev → v2', async () => {
  const dir = makeTmp('remix-detect-vite-');
  write(dir, 'vite.config.ts', "import { vitePlugin as remix } from '@remix-run/dev';");
  write(dir, 'app/routes/users/index.tsx', ''); // even with nested dirs
  const c = await detectConvention(dir, path.join(dir, 'app/routes'));
  assert.equal(c, 'v2-flat');
});
