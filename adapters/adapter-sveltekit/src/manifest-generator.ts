import {
  detectMethods,
  detectPageServerMethods,
  type HttpMethod,
} from './method-detector';
import type { ScannedRoute } from './route-scanner';

export interface RouteEntry {
  path: string;
  handler: string;
  methods: HttpMethod[];
}

export interface RouteManifest {
  version: 1;
  routes: RouteEntry[];
  static: string[];
  capabilities?: Record<string, string[]>;
}

export interface ManifestOptions {
  staticPatterns?: string[];
  capabilities?: Record<string, string[]>;
  /** When true, include `+page.server.{ts,js}` routes (form actions / load).
   *  Defaults to true. */
  includePageServer?: boolean;
  /** Override / supply method detection (skip filesystem reads — for tests). */
  methodOverrides?: Record<string, HttpMethod[]>;
}

const DEFAULT_METHODS: HttpMethod[] = ['GET'];

export async function generateManifest(
  routes: ScannedRoute[],
  options?: ManifestOptions
): Promise<RouteManifest> {
  const includePageServer = options?.includePageServer !== false;
  const overrides = options?.methodOverrides ?? {};

  const manifestRoutes: RouteEntry[] = [];
  const seen = new Set<string>();

  for (const route of routes) {
    if (route.type === 'layout' || route.type === 'page') continue;
    if (route.type === 'page-server' && !includePageServer) continue;

    let methods: HttpMethod[];
    if (overrides[route.filePath]) {
      methods = overrides[route.filePath];
    } else if (route.type === 'api') {
      const detected = await detectMethods(route.filePath);
      methods = detected.length > 0 ? detected : DEFAULT_METHODS;
    } else {
      const detected = await detectPageServerMethods(route.filePath);
      methods = detected.length > 0 ? detected : DEFAULT_METHODS;
    }

    const handlerName = routeToHandlerName(route.path);
    const handler = `${handlerName}.wasm`;

    const dedupKey = `${route.path}::${handler}`;
    if (seen.has(dedupKey)) continue;
    seen.add(dedupKey);

    manifestRoutes.push({
      path: route.path,
      handler,
      methods: dedupeMethods(methods),
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

/** Convert a route path to a safe bare handler filename.
 *  `/api/users/:id` → `api-users-id` ; `/` → `index` ; `/api/*` → `api`.
 *  Result is sanitized for the gateway parser (no `/`, `\\`, `..`, no empty).
 */
export function routeToHandlerName(routePath: string): string {
  let name = routePath
    .replace(/^\/+/, '')
    .replace(/\*/g, '')
    .replace(/[/\\:]+/g, '-')
    .replace(/\.+/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-+|-+$/g, '');

  if (name === '' || name === '.' || name === '..') name = 'index';
  return name;
}

function dedupeMethods(methods: HttpMethod[]): HttpMethod[] {
  const order: HttpMethod[] = ['GET', 'POST', 'PUT', 'DELETE', 'PATCH', 'HEAD', 'OPTIONS'];
  const seen = new Set(methods);
  return order.filter((m) => seen.has(m));
}
