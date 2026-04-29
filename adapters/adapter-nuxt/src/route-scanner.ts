import * as fs from 'fs';
import * as path from 'path';

export type RouteKind = 'api' | 'route' | 'middleware';

export type HttpMethod =
  | 'GET'
  | 'POST'
  | 'PUT'
  | 'DELETE'
  | 'PATCH'
  | 'HEAD'
  | 'OPTIONS';

export interface ScannedRoute {
  path: string;
  type: RouteKind;
  filePath: string;
  isDynamic: boolean;
  methods?: HttpMethod[];
}

const ROUTE_EXTENSIONS = ['.ts', '.mts', '.cts', '.js', '.mjs', '.cjs'];

const METHOD_SUFFIXES: Record<string, HttpMethod> = {
  get: 'GET',
  post: 'POST',
  put: 'PUT',
  delete: 'DELETE',
  patch: 'PATCH',
  head: 'HEAD',
  options: 'OPTIONS',
};

interface ScanRoot {
  dir: string;
  prefix: string;
  type: RouteKind;
}

export async function scanRoutes(projectDir: string): Promise<ScannedRoute[]> {
  const serverDir = path.join(projectDir, 'server');
  if (!fs.existsSync(serverDir)) return [];

  const roots: ScanRoot[] = [
    { dir: path.join(serverDir, 'api'), prefix: '/api', type: 'api' },
    { dir: path.join(serverDir, 'routes'), prefix: '', type: 'route' },
    {
      dir: path.join(serverDir, 'middleware'),
      prefix: '',
      type: 'middleware',
    },
  ];

  const collected: ScannedRoute[] = [];
  for (const root of roots) {
    if (!fs.existsSync(root.dir)) continue;
    await walk(root.dir, root.prefix, root.type, collected);
  }

  return mergeMethodVariants(collected);
}

async function walk(
  dir: string,
  urlPrefix: string,
  type: RouteKind,
  out: ScannedRoute[]
): Promise<void> {
  const entries = await fs.promises.readdir(dir, { withFileTypes: true });

  for (const entry of entries) {
    if (entry.name.startsWith('_') || entry.name.startsWith('.')) continue;

    const fullPath = path.join(dir, entry.name);

    if (entry.isDirectory()) {
      const segment = dirToSegment(entry.name);
      const nextPrefix =
        segment === '' ? urlPrefix : `${urlPrefix}/${segment}`;
      await walk(fullPath, nextPrefix, type, out);
      continue;
    }

    if (!entry.isFile()) continue;

    const ext = path.extname(entry.name);
    if (!ROUTE_EXTENSIONS.includes(ext)) continue;

    const baseName = entry.name.slice(0, entry.name.length - ext.length);
    const { stem, method } = splitMethodSuffix(baseName);

    if (stem.startsWith('_') || stem.startsWith('.') || stem === '') continue;

    const segment = fileStemToSegment(stem);
    const routePath = buildRoutePath(urlPrefix, segment);

    out.push({
      path: routePath,
      type,
      filePath: fullPath,
      isDynamic: routePath.includes(':') || routePath.includes('*'),
      methods: type === 'middleware' || !method ? undefined : [method],
    });
  }
}

function buildRoutePath(prefix: string, segment: string): string {
  if (segment === '') return prefix === '' ? '/' : prefix;
  return `${prefix}/${segment}`;
}

function splitMethodSuffix(baseName: string): {
  stem: string;
  method?: HttpMethod;
} {
  const dot = baseName.lastIndexOf('.');
  if (dot <= 0) return { stem: baseName };
  const suffix = baseName.slice(dot + 1).toLowerCase();
  const method = METHOD_SUFFIXES[suffix];
  if (!method) return { stem: baseName };
  return { stem: baseName.slice(0, dot), method };
}

function fileStemToSegment(stem: string): string {
  if (stem === 'index') return '';
  return dirToSegment(stem);
}

function dirToSegment(name: string): string {
  if (name.startsWith('(') && name.endsWith(')')) return '';
  if (name.startsWith('[...') && name.endsWith(']')) return '*';
  if (name.startsWith('[') && name.endsWith(']')) {
    return ':' + name.slice(1, -1);
  }
  return name;
}

function mergeMethodVariants(routes: ScannedRoute[]): ScannedRoute[] {
  const merged = new Map<string, ScannedRoute>();
  const passThrough: ScannedRoute[] = [];

  for (const route of routes) {
    if (route.type === 'middleware') {
      passThrough.push(route);
      continue;
    }

    const key = `${route.type}::${route.path}`;
    const existing = merged.get(key);

    if (!existing) {
      merged.set(key, {
        ...route,
        methods: route.methods ? [...route.methods] : undefined,
      });
      continue;
    }

    if (!existing.methods || !route.methods) {
      existing.methods = undefined;
      continue;
    }

    for (const m of route.methods) {
      if (!existing.methods.includes(m)) existing.methods.push(m);
    }
  }

  return [...merged.values(), ...passThrough];
}
