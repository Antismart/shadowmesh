import type { ScannedRoute } from './route-scanner';
import { detectMethods, type HttpMethod } from './method-detector';

export interface RouteManifestEntry {
  path: string;
  handler: string;
  methods: HttpMethod[];
}

export interface RouteManifest {
  version: 1;
  routes: RouteManifestEntry[];
  static: string[];
  capabilities?: Record<string, string[]>;
}

export interface ManifestOptions {
  staticPatterns?: string[];
  capabilities?: Record<string, string[]>;
  /** Override or extend the handler suffix; defaults to `.wasm`. */
  handlerExtension?: string;
}

export async function generateManifest(
  routes: ScannedRoute[],
  options: ManifestOptions = {}
): Promise<RouteManifest> {
  const ext = normalizeExtension(options.handlerExtension ?? '.wasm');
  const entries: RouteManifestEntry[] = [];
  const usedHandlers = new Set<string>();

  for (const route of routes) {
    const methods = (await detectMethods(route.filePath)).methods;
    const handler = uniqueHandler(routeToHandlerStem(route.path), ext, usedHandlers);
    entries.push({
      path: route.path,
      handler,
      methods,
    });
  }

  const manifest: RouteManifest = {
    version: 1,
    routes: entries,
    static: options.staticPatterns ?? ['/**'],
  };
  if (options.capabilities) manifest.capabilities = options.capabilities;
  return manifest;
}

/**
 * Convert a route path into a safe handler stem.
 * Examples:
 *   /                  → index
 *   /users/:id         → users-id
 *   /blog/*            → blog-splat
 *   /api/users/:id     → api-users-id
 */
export function routeToHandlerStem(routePath: string): string {
  const cleaned = routePath
    .replace(/^\//, '')
    .split('/')
    .map((seg) => {
      if (seg === '*') return 'splat';
      if (seg.startsWith(':')) return seg.slice(1);
      return seg;
    })
    .filter((s) => s.length > 0)
    .join('-')
    .replace(/[^a-zA-Z0-9_-]/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-+|-+$/g, '');

  if (!cleaned || cleaned === '..' || cleaned.includes('..')) return 'index';
  return cleaned || 'index';
}

function uniqueHandler(stem: string, ext: string, used: Set<string>): string {
  let candidate = `${stem}${ext}`;
  let n = 2;
  while (used.has(candidate)) {
    candidate = `${stem}-${n}${ext}`;
    n++;
  }
  used.add(candidate);
  return candidate;
}

function normalizeExtension(ext: string): string {
  if (!ext) return '.wasm';
  return ext.startsWith('.') ? ext : `.${ext}`;
}
