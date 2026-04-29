import * as fs from 'fs';

export type HttpMethod = 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH' | 'HEAD' | 'OPTIONS';

const VALID_METHODS: ReadonlySet<HttpMethod> = new Set([
  'GET',
  'POST',
  'PUT',
  'DELETE',
  'PATCH',
  'HEAD',
  'OPTIONS',
]);

const METHOD_EXPORT_RE =
  /export\s+(?:const|let|var|async\s+function|function)\s+(GET|POST|PUT|DELETE|PATCH|HEAD|OPTIONS)\b/g;

const RE_EXPORT_RE =
  /export\s*\{\s*([^}]+)\s*\}/g;

/** Extract HTTP methods declared in a SvelteKit `+server.{ts,js}` file.
 *  Uses a lightweight regex pass — does not parse TypeScript. Sufficient for
 *  the adapter's manifest emission, but will miss exotic export patterns
 *  (re-exports from a non-direct source, computed names, etc.).
 */
export function detectMethodsFromSource(source: string): HttpMethod[] {
  const found = new Set<HttpMethod>();

  for (const match of source.matchAll(METHOD_EXPORT_RE)) {
    const m = match[1] as HttpMethod;
    if (VALID_METHODS.has(m)) found.add(m);
  }

  for (const match of source.matchAll(RE_EXPORT_RE)) {
    const inside = match[1];
    for (const part of inside.split(',')) {
      const trimmed = part.trim();
      if (!trimmed) continue;
      const segments = trimmed.split(/\s+as\s+/);
      const exported = (segments[1] ?? segments[0]).trim().toUpperCase();
      if (exported && VALID_METHODS.has(exported as HttpMethod)) {
        found.add(exported as HttpMethod);
      }
    }
  }

  return Array.from(found);
}

export async function detectMethods(filePath: string): Promise<HttpMethod[]> {
  const source = await fs.promises.readFile(filePath, 'utf8');
  return detectMethodsFromSource(source);
}

/** For `+page.server.{ts,js}` files: GET if `load` is exported, POST if
 *  `actions` is exported. SvelteKit form actions are POST-only.
 */
export function detectPageServerMethodsFromSource(source: string): HttpMethod[] {
  const methods = new Set<HttpMethod>();
  if (/export\s+(?:const|let|var|async\s+function|function)\s+load\b/.test(source)) {
    methods.add('GET');
  }
  if (/export\s+(?:const|let|var)\s+actions\b/.test(source)) {
    methods.add('POST');
  }
  return Array.from(methods);
}

export async function detectPageServerMethods(filePath: string): Promise<HttpMethod[]> {
  const source = await fs.promises.readFile(filePath, 'utf8');
  return detectPageServerMethodsFromSource(source);
}
