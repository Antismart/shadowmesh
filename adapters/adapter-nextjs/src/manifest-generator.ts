import type { ScannedRoute } from './route-scanner';

interface RouteEntry {
  path: string;
  handler: string;
  methods?: string[];
}

interface RouteManifest {
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

export function generateManifest(
  routes: ScannedRoute[],
  options?: ManifestOptions
): RouteManifest {
  const manifestRoutes: RouteEntry[] = [];

  // Filter to API routes — these become edge function handlers
  const apiRoutes = routes.filter((r) => r.type === 'api');

  for (const route of apiRoutes) {
    // Check if this route matches edge route patterns
    if (options?.edgeRoutes && options.edgeRoutes.length > 0) {
      const isEdge = options.edgeRoutes.some((pattern) =>
        matchPattern(route.path, pattern)
      );
      if (!isEdge) continue;
    }

    const handlerName = routeToHandlerName(route.path);

    manifestRoutes.push({
      path: route.isDynamic ? route.path : route.path,
      handler: `${handlerName}.wasm`,
      methods: ['GET', 'POST', 'PUT', 'DELETE', 'PATCH'],
    });
  }

  // If no specific edge routes configured, add a catch-all for /api/*
  if (manifestRoutes.length === 0 && apiRoutes.length > 0) {
    manifestRoutes.push({
      path: '/api/*',
      handler: 'api.wasm',
      methods: ['GET', 'POST', 'PUT', 'DELETE', 'PATCH'],
    });
  }

  return {
    version: 1,
    routes: manifestRoutes,
    static: options?.staticPatterns || ['/**'],
    capabilities: options?.capabilities,
  };
}

function routeToHandlerName(routePath: string): string {
  return routePath
    .replace(/^\//, '')
    .replace(/[/:*]/g, '-')
    .replace(/-+/g, '-')
    .replace(/-$/, '') || 'index';
}

function matchPattern(path: string, pattern: string): boolean {
  if (pattern === path) return true;
  if (pattern.endsWith('/*')) {
    const prefix = pattern.slice(0, -2);
    return path === prefix || path.startsWith(prefix + '/');
  }
  return false;
}
