import type { HttpMethod, ScannedRoute } from './route-scanner';

interface RouteEntry {
  path: string;
  handler: string;
  methods?: HttpMethod[];
}

export interface RouteManifest {
  version: 1;
  routes: RouteEntry[];
  static?: string[];
  capabilities?: Record<string, string[]>;
}

export interface ManifestOptions {
  edgeRoutes?: string[];
  staticPatterns?: string[];
  capabilities?: Record<string, string[]>;
}

const DEFAULT_METHODS: HttpMethod[] = [
  'GET',
  'POST',
  'PUT',
  'DELETE',
  'PATCH',
];

export function generateManifest(
  routes: ScannedRoute[],
  options?: ManifestOptions
): RouteManifest {
  const handlerRoutes = routes.filter(
    (r) => r.type === 'api' || r.type === 'route'
  );

  const seenHandlers = new Set<string>();
  const manifestRoutes: RouteEntry[] = [];

  for (const route of handlerRoutes) {
    if (options?.edgeRoutes && options.edgeRoutes.length > 0) {
      const isEdge = options.edgeRoutes.some((pattern) =>
        matchPattern(route.path, pattern)
      );
      if (!isEdge) continue;
    }

    const handler = uniqueHandler(
      `${routeToHandlerName(route.path)}.wasm`,
      seenHandlers
    );

    manifestRoutes.push({
      path: route.path,
      handler,
      methods: route.methods && route.methods.length > 0
        ? [...route.methods]
        : [...DEFAULT_METHODS],
    });
  }

  const manifest: RouteManifest = {
    version: 1,
    routes: manifestRoutes,
    static: options?.staticPatterns ?? ['/**'],
  };

  if (options?.capabilities && Object.keys(options.capabilities).length > 0) {
    manifest.capabilities = options.capabilities;
  }

  return manifest;
}

export function routeToHandlerName(routePath: string): string {
  const cleaned = routePath
    .replace(/^\/+/, '')
    .replace(/[/:*]+/g, '-')
    .replace(/[^A-Za-z0-9._-]/g, '-')
    .replace(/-+/g, '-')
    .replace(/^[-.]+/, '')
    .replace(/[-.]+$/, '');

  if (!cleaned || cleaned.includes('..')) return 'index';
  return cleaned;
}

function uniqueHandler(name: string, seen: Set<string>): string {
  if (!seen.has(name)) {
    seen.add(name);
    return name;
  }
  const dot = name.lastIndexOf('.');
  const stem = dot === -1 ? name : name.slice(0, dot);
  const ext = dot === -1 ? '' : name.slice(dot);
  let i = 2;
  while (seen.has(`${stem}-${i}${ext}`)) i++;
  const next = `${stem}-${i}${ext}`;
  seen.add(next);
  return next;
}

function matchPattern(targetPath: string, pattern: string): boolean {
  if (pattern === targetPath) return true;
  if (pattern.endsWith('/*')) {
    const prefix = pattern.slice(0, -2);
    return targetPath === prefix || targetPath.startsWith(prefix + '/');
  }
  return false;
}
