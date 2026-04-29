import * as fs from 'fs';

export type HttpMethod = 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH' | 'HEAD' | 'OPTIONS';

/** Methods Remix dispatches to a route's `action` export. */
const ACTION_METHODS: HttpMethod[] = ['POST', 'PUT', 'DELETE', 'PATCH'];

/**
 * Single-pass regex over the route module source. Catches:
 *   export const loader = ...
 *   export let loader = ...
 *   export function loader(...)
 *   export async function loader(...)
 *   export { loader }   /  export { x as loader }
 *
 * Intentional simplification: we don't parse TS/JS into an AST. False positives
 * on commented-out exports would just inflate the method set, which is a no-op
 * at the gateway. We accept that trade for build-time speed.
 */
const NAMED_EXPORT_RE =
  /export\s+(?:const|let|var|async\s+function|function)\s+(loader|action)\b/g;

const REEXPORT_RE =
  /export\s*\{\s*([^}]+)\s*\}/g;

export interface DetectedMethods {
  hasLoader: boolean;
  hasAction: boolean;
  methods: HttpMethod[];
}

export async function detectMethods(filePath: string): Promise<DetectedMethods> {
  let source: string;
  try {
    source = await fs.promises.readFile(filePath, 'utf8');
  } catch {
    return { hasLoader: false, hasAction: false, methods: ['GET'] };
  }
  return detectMethodsFromSource(source);
}

export function detectMethodsFromSource(source: string): DetectedMethods {
  // Strip line + block comments cheaply so commented-out exports don't count.
  const stripped = source
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/(^|[^:])\/\/.*$/gm, '$1');

  let hasLoader = false;
  let hasAction = false;

  for (const match of stripped.matchAll(NAMED_EXPORT_RE)) {
    if (match[1] === 'loader') hasLoader = true;
    if (match[1] === 'action') hasAction = true;
  }

  for (const match of stripped.matchAll(REEXPORT_RE)) {
    const names = match[1].split(',').map((n) => n.trim());
    for (const n of names) {
      // Handle `x as loader` aliasing.
      const aliased = /\bas\s+(\w+)$/.exec(n);
      const exported = aliased ? aliased[1] : n;
      if (exported === 'loader') hasLoader = true;
      if (exported === 'action') hasAction = true;
    }
  }

  const set = new Set<HttpMethod>();
  if (hasLoader) set.add('GET');
  if (hasAction) ACTION_METHODS.forEach((m) => set.add(m));
  if (set.size === 0) set.add('GET');

  return {
    hasLoader,
    hasAction,
    methods: orderMethods(set),
  };
}

const METHOD_ORDER: HttpMethod[] = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'];

function orderMethods(set: Set<HttpMethod>): HttpMethod[] {
  return METHOD_ORDER.filter((m) => set.has(m));
}
