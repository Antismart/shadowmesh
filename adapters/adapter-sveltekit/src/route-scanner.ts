import * as fs from 'fs';
import * as path from 'path';

export type RouteType = 'page' | 'api' | 'layout' | 'page-server';

export interface ScannedRoute {
  path: string;
  type: RouteType;
  filePath: string;
  isDynamic: boolean;
}

const SCRIPT_EXTENSIONS = ['.ts', '.js', '.tsx', '.jsx'];
const SVELTE_EXTENSION = '.svelte';

export interface ScanOptions {
  routesDir?: string;
}

export async function scanRoutes(
  projectDir: string,
  options?: ScanOptions
): Promise<ScannedRoute[]> {
  const routesDir = options?.routesDir
    ? path.resolve(projectDir, options.routesDir)
    : path.join(projectDir, 'src', 'routes');

  if (!fs.existsSync(routesDir)) return [];

  const routes: ScannedRoute[] = [];
  await walk(routesDir, '', routes);
  return routes;
}

async function walk(dir: string, urlPrefix: string, routes: ScannedRoute[]): Promise<void> {
  const entries = await fs.promises.readdir(dir, { withFileTypes: true });

  for (const entry of entries) {
    if (entry.name.startsWith('_') || entry.name.startsWith('.')) continue;

    const fullPath = path.join(dir, entry.name);

    if (entry.isDirectory()) {
      const segment = dirToSegment(entry.name);
      const nextPrefix = segment === '' ? urlPrefix : `${urlPrefix}/${segment}`;
      await walk(fullPath, nextPrefix, routes);
      continue;
    }

    const parsed = path.parse(entry.name);
    if (!parsed.name.startsWith('+')) continue;

    const baseName = parsed.name;
    const ext = parsed.ext;

    const routePath = normalizePath(urlPrefix);

    if (baseName === '+server' && SCRIPT_EXTENSIONS.includes(ext)) {
      routes.push({
        path: routePath,
        type: 'api',
        filePath: fullPath,
        isDynamic: routePath.includes(':') || routePath.includes('*'),
      });
    } else if (baseName === '+page' && ext === SVELTE_EXTENSION) {
      routes.push({
        path: routePath,
        type: 'page',
        filePath: fullPath,
        isDynamic: routePath.includes(':') || routePath.includes('*'),
      });
    } else if (baseName === '+page.server' && SCRIPT_EXTENSIONS.includes(ext)) {
      routes.push({
        path: routePath,
        type: 'page-server',
        filePath: fullPath,
        isDynamic: routePath.includes(':') || routePath.includes('*'),
      });
    } else if (baseName === '+layout' && ext === SVELTE_EXTENSION) {
      routes.push({
        path: routePath,
        type: 'layout',
        filePath: fullPath,
        isDynamic: routePath.includes(':') || routePath.includes('*'),
      });
    }
  }
}

function normalizePath(urlPrefix: string): string {
  if (urlPrefix === '' || urlPrefix === '/') return '/';
  return urlPrefix.replace(/\/+/g, '/');
}

/** Convert a SvelteKit directory name to a URL segment.
 *  - `(group)` → '' (route group, stripped)
 *  - `[...slug]` → '*' (catch-all rest param)
 *  - `[id=integer]` → ':id' (param with matcher — matcher dropped)
 *  - `[id]` → ':id' (dynamic segment)
 *  - `foo` → 'foo'
 */
export function dirToSegment(name: string): string {
  if (name.startsWith('(') && name.endsWith(')')) return '';

  if (name.startsWith('[...') && name.endsWith(']')) {
    return '*';
  }

  if (name.startsWith('[[') && name.endsWith(']]')) {
    const inner = name.slice(2, -2);
    const paramName = stripMatcher(inner);
    return ':' + paramName;
  }

  if (name.startsWith('[') && name.endsWith(']')) {
    const inner = name.slice(1, -1);
    const paramName = stripMatcher(inner);
    return ':' + paramName;
  }

  return name;
}

function stripMatcher(inner: string): string {
  const eq = inner.indexOf('=');
  return eq === -1 ? inner : inner.slice(0, eq);
}
