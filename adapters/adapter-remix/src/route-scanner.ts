import * as fs from 'fs';
import * as path from 'path';

export type RouteConvention = 'v2-flat' | 'v1-nested';

export interface ScannedRoute {
  /** URL path with `:param` and `*` syntax (gateway-compatible). */
  path: string;
  /** Absolute path to the route module on disk. */
  filePath: string;
  /** True for index routes (`_index` v2, `index.tsx` v1). */
  isIndex: boolean;
  /** True if path ends in catch-all `*`. */
  isSplat: boolean;
  /** True if path contains a `:param` segment. */
  isDynamic: boolean;
}

const ROUTE_EXTENSIONS = new Set(['.ts', '.tsx', '.js', '.jsx', '.mjs', '.cjs']);

export interface ScanOptions {
  /** Force a convention; auto-detected when omitted. */
  convention?: RouteConvention;
  /** App directory; defaults to `<projectDir>/app`. */
  appDir?: string;
}

export async function scanRoutes(
  projectDir: string,
  options: ScanOptions = {}
): Promise<ScannedRoute[]> {
  const appDir = options.appDir ?? path.join(projectDir, 'app');
  const routesDir = path.join(appDir, 'routes');

  if (!fs.existsSync(routesDir)) return [];

  const convention = options.convention ?? (await detectConvention(projectDir, routesDir));
  const routes: ScannedRoute[] = [];

  if (convention === 'v2-flat') {
    await scanV2Flat(routesDir, routes);
  } else {
    await scanV1Nested(routesDir, [], routes);
  }

  // Stable order: index/exact paths before splats, then alpha
  routes.sort((a, b) => {
    if (a.isSplat !== b.isSplat) return a.isSplat ? 1 : -1;
    return a.path.localeCompare(b.path);
  });

  return routes;
}

export async function detectConvention(
  projectDir: string,
  routesDir: string
): Promise<RouteConvention> {
  // Explicit opt-out via remix.config.js → v1
  const remixConfigPaths = [
    path.join(projectDir, 'remix.config.js'),
    path.join(projectDir, 'remix.config.mjs'),
    path.join(projectDir, 'remix.config.cjs'),
    path.join(projectDir, 'remix.config.ts'),
  ];
  for (const p of remixConfigPaths) {
    if (!fs.existsSync(p)) continue;
    try {
      const text = await fs.promises.readFile(p, 'utf8');
      if (/v3_routeConvention\s*[:=]\s*false/.test(text)) return 'v1-nested';
      if (/v3_routeConvention\s*[:=]\s*true/.test(text)) return 'v2-flat';
    } catch {
      /* ignore */
    }
  }

  // Vite config referencing @remix-run/dev/vite → v2
  const viteConfigs = ['vite.config.ts', 'vite.config.js', 'vite.config.mjs'];
  for (const v of viteConfigs) {
    const p = path.join(projectDir, v);
    if (!fs.existsSync(p)) continue;
    try {
      const text = await fs.promises.readFile(p, 'utf8');
      if (text.includes('@remix-run/dev')) return 'v2-flat';
    } catch {
      /* ignore */
    }
  }

  // Heuristic: directories present under routes/ → v1; otherwise v2.
  const entries = await fs.promises.readdir(routesDir, { withFileTypes: true });
  const hasNestedDirs = entries.some((e) => e.isDirectory() && !e.name.startsWith('.'));
  return hasNestedDirs ? 'v1-nested' : 'v2-flat';
}

async function scanV2Flat(routesDir: string, out: ScannedRoute[]): Promise<void> {
  const entries = await fs.promises.readdir(routesDir, { withFileTypes: true });
  for (const entry of entries) {
    if (entry.name.startsWith('.')) continue;
    const full = path.join(routesDir, entry.name);

    if (entry.isDirectory()) {
      // v2 supports a folder containing route.tsx for colocation
      const route = await readV2Folder(full, entry.name);
      if (route) out.push(route);
      continue;
    }

    const parsed = path.parse(entry.name);
    if (!ROUTE_EXTENSIONS.has(parsed.ext)) continue;

    const skipReason = v2SkipReason(parsed.name);
    if (skipReason) continue;

    const urlPath = v2FilenameToPath(parsed.name);
    if (urlPath === null) continue;

    out.push(buildRoute(urlPath, full, parsed.name === '_index'));
  }
}

async function readV2Folder(folder: string, folderName: string): Promise<ScannedRoute | null> {
  if (v2SkipReason(folderName)) return null;
  const candidates = ['route.ts', 'route.tsx', 'route.js', 'route.jsx', 'index.ts', 'index.tsx'];
  for (const c of candidates) {
    const p = path.join(folder, c);
    if (fs.existsSync(p)) {
      const urlPath = v2FilenameToPath(folderName);
      if (urlPath === null) return null;
      return buildRoute(urlPath, p, folderName === '_index');
    }
  }
  return null;
}

/**
 * Reasons to skip a v2 route name:
 *  - leading `.` (handled by caller)
 *  - leading `_` that is *not* `_index` (pathless layout, no own URL)
 *  - empty
 */
function v2SkipReason(name: string): string | null {
  if (!name) return 'empty';
  if (name === '_index') return null;
  // Pathless layout files: `_marketing.tsx` (no segments after `_marketing`)
  // versus pathless layout *prefix*: `_marketing.about.tsx` — that one is a real route.
  if (name.startsWith('_') && !name.includes('.')) return 'pathless-layout';
  return null;
}

/** Convert a v2 flat-file basename (without extension) into a URL path. */
export function v2FilenameToPath(name: string): string | null {
  if (name === '_index') return '/';

  const segments = splitV2Segments(name);
  const out: string[] = [];

  for (let i = 0; i < segments.length; i++) {
    const seg = segments[i];
    const isLast = i === segments.length - 1;

    // Pathless layout prefix: `_marketing` — strip it.
    if (seg.startsWith('_') && seg !== '_index') {
      continue;
    }

    // Index marker mid-pattern: `users._index` → trailing index of /users.
    if (seg === '_index' && isLast) {
      continue;
    }

    // Optional segment: `($lang)` — emit *without* the optional segment.
    if (seg.startsWith('(') && seg.endsWith(')')) {
      continue;
    }

    // Trailing-underscore segment: `users_` → `users` (strips parent layout, same URL).
    let cleaned = seg.endsWith('_') ? seg.slice(0, -1) : seg;

    // Splat: `$` alone → `*` (must be the last segment).
    if (cleaned === '$') {
      if (!isLast) return null;
      out.push('*');
      continue;
    }

    // Dynamic: `$id` → `:id`.
    if (cleaned.startsWith('$')) {
      out.push(':' + cleaned.slice(1));
      continue;
    }

    // Escaped literal: `[.]` keeps the dot, `[$]` keeps the dollar.
    cleaned = cleaned.replace(/\[(.)\]/g, '$1');

    if (cleaned === '') continue;
    out.push(cleaned);
  }

  if (out.length === 0) return '/';
  return '/' + out.join('/');
}

/** Split a v2 filename on unescaped dots. `[.]` stays literal. */
function splitV2Segments(name: string): string[] {
  const segs: string[] = [];
  let buf = '';
  let i = 0;
  while (i < name.length) {
    const ch = name[i];
    if (ch === '[' && i + 2 < name.length && name[i + 2] === ']') {
      buf += name.slice(i, i + 3);
      i += 3;
      continue;
    }
    if (ch === '.') {
      segs.push(buf);
      buf = '';
      i++;
      continue;
    }
    buf += ch;
    i++;
  }
  if (buf) segs.push(buf);
  return segs;
}

async function scanV1Nested(
  baseDir: string,
  urlSegments: string[],
  out: ScannedRoute[]
): Promise<void> {
  const entries = await fs.promises.readdir(baseDir, { withFileTypes: true });

  for (const entry of entries) {
    if (entry.name.startsWith('.')) continue;
    const full = path.join(baseDir, entry.name);

    if (entry.isDirectory()) {
      // Pathless layout dir: `__marketing` — Remix v1 used double-underscore.
      // We treat single or double leading underscore as pathless.
      if (entry.name.startsWith('_')) {
        await scanV1Nested(full, urlSegments, out);
        continue;
      }
      const seg = v1NameToSegment(entry.name);
      if (seg === null) continue;
      const next = seg === '' ? urlSegments : [...urlSegments, seg];
      await scanV1Nested(full, next, out);
      continue;
    }

    const parsed = path.parse(entry.name);
    if (!ROUTE_EXTENSIONS.has(parsed.ext)) continue;
    if (parsed.name.startsWith('.')) continue;
    // Skip pathless layout files like `__app.tsx` at any level.
    if (parsed.name.startsWith('_')) continue;

    const isIndex = parsed.name === 'index';
    let segments = urlSegments;
    if (!isIndex) {
      const seg = v1NameToSegment(parsed.name);
      if (seg === null) continue;
      if (seg !== '') segments = [...urlSegments, seg];
    }

    const isSplatLast = segments.length > 0 && segments[segments.length - 1] === '*';
    const urlPath = segments.length === 0 ? '/' : '/' + segments.join('/');
    out.push({
      path: urlPath,
      filePath: full,
      isIndex,
      isSplat: isSplatLast,
      isDynamic: segments.some((s) => s.startsWith(':')),
    });
  }
}

function v1NameToSegment(name: string): string | null {
  if (name === '$') return '*';
  if (name.startsWith('$')) return ':' + name.slice(1);
  // Escaped literal forms like `[.json]`
  return name.replace(/\[(.)\]/g, '$1');
}

function buildRoute(urlPath: string, filePath: string, isIndex: boolean): ScannedRoute {
  return {
    path: urlPath,
    filePath,
    isIndex,
    isSplat: urlPath.endsWith('/*') || urlPath === '/*',
    isDynamic: /\/:[^/]+/.test(urlPath),
  };
}
