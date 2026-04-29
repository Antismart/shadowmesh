import * as fs from 'fs';
import * as path from 'path';
import type { Adapter, Builder } from '@sveltejs/kit';
import { scanRoutes, type ScannedRoute } from './route-scanner';
import { generateManifest, type ManifestOptions } from './manifest-generator';
import {
  detectMethodsFromSource,
  detectPageServerMethodsFromSource,
  type HttpMethod,
} from './method-detector';

export interface ShadowMeshAdapterOptions {
  /** Output directory (defaults to `build`). */
  out?: string;
  /** Static glob patterns to declare in the manifest. Defaults to `['/**']`. */
  staticPatterns?: string[];
  /** Manifest `capabilities` map keyed by handler filename. */
  capabilities?: Record<string, string[]>;
  /** Skip writing the static client/prerendered/server output (for users who
   *  delegate that to another adapter). When false (default) we mirror the
   *  output Kit's adapter-static produces. */
  skipKitOutput?: boolean;
  /** Force a filesystem rescan of `src/routes` instead of using `builder.routes`. */
  forceFilesystemScan?: boolean;
}

export default function adapter(options: ShadowMeshAdapterOptions = {}): Adapter {
  return {
    name: '@shadowmesh/adapter-sveltekit',
    async adapt(builder: Builder): Promise<void> {
      const out = options.out ?? 'build';

      if (!options.skipKitOutput) {
        builder.rimraf(out);
        builder.mkdirp(out);
        builder.writeClient(`${out}/client`);
        builder.writePrerendered(`${out}/prerendered`);
        builder.writeServer(`${out}/server`);
      }

      const projectDir = process.cwd();
      const useBuilderRoutes =
        !options.forceFilesystemScan && hasResolvedRoutes(builder);

      const scanned: ScannedRoute[] = useBuilderRoutes
        ? routesFromBuilder(builder)
        : await scanRoutes(projectDir);

      const manifestOptions: ManifestOptions = {
        staticPatterns: options.staticPatterns,
        capabilities: options.capabilities,
        methodOverrides: useBuilderRoutes
          ? methodOverridesFromBuilder(builder)
          : undefined,
      };

      const manifest = await generateManifest(scanned, manifestOptions);

      const smDir = path.join(out, '_shadowmesh');
      builder.mkdirp(smDir);
      const manifestPath = path.join(smDir, 'routes.json');
      await fs.promises.writeFile(manifestPath, JSON.stringify(manifest, null, 2));

      builder.log.success(
        `[shadowmesh] wrote ${manifest.routes.length} route(s) to ${manifestPath}`
      );
    },
  };
}

interface BuilderRouteData {
  id: string;
  endpoint?: { file: string } | null;
  page?: { file: string } | null;
}

function hasResolvedRoutes(builder: Builder): boolean {
  const candidate = builder as unknown as { routes?: unknown };
  return Array.isArray(candidate.routes) && candidate.routes.length >= 0;
}

function routesFromBuilder(builder: Builder): ScannedRoute[] {
  const raw = (builder as unknown as { routes?: BuilderRouteData[] }).routes;
  if (!Array.isArray(raw)) return [];

  const out: ScannedRoute[] = [];
  for (const r of raw) {
    if (!r || typeof r.id !== 'string') continue;
    const routePath = builderIdToPath(r.id);
    const isDynamic = routePath.includes(':') || routePath.includes('*');

    if (r.endpoint && r.endpoint.file) {
      out.push({
        path: routePath,
        type: 'api',
        filePath: r.endpoint.file,
        isDynamic,
      });
    }
    if (r.page && r.page.file) {
      out.push({
        path: routePath,
        type: 'page',
        filePath: r.page.file,
        isDynamic,
      });
    }
  }
  return out;
}

function methodOverridesFromBuilder(builder: Builder): Record<string, HttpMethod[]> {
  const raw = (builder as unknown as { routes?: BuilderRouteData[] }).routes;
  const out: Record<string, HttpMethod[]> = {};
  if (!Array.isArray(raw)) return out;

  for (const r of raw) {
    const file = r?.endpoint?.file;
    if (file && fs.existsSync(file)) {
      try {
        const src = fs.readFileSync(file, 'utf8');
        const detected = detectMethodsFromSource(src);
        if (detected.length > 0) out[file] = detected;
      } catch {
        // ignore — fall through to default
      }
    }
    const pageFile = r?.page?.file;
    if (pageFile && /\+page\.server\.[tj]sx?$/.test(pageFile) && fs.existsSync(pageFile)) {
      try {
        const src = fs.readFileSync(pageFile, 'utf8');
        const detected = detectPageServerMethodsFromSource(src);
        if (detected.length > 0) out[pageFile] = detected;
      } catch {
        // ignore
      }
    }
  }
  return out;
}

/** Convert a SvelteKit route id (e.g. `/api/users/[id=integer]`) to manifest
 *  path syntax (`/api/users/:id`). */
export function builderIdToPath(id: string): string {
  if (id === '' || id === '/') return '/';

  const parts = id.split('/').filter((p) => p.length > 0);
  const out: string[] = [];

  for (const part of parts) {
    if (part.startsWith('(') && part.endsWith(')')) continue;
    if (part.startsWith('[...') && part.endsWith(']')) {
      out.push('*');
      continue;
    }
    if (part.startsWith('[[') && part.endsWith(']]')) {
      const inner = part.slice(2, -2);
      const eq = inner.indexOf('=');
      out.push(':' + (eq === -1 ? inner : inner.slice(0, eq)));
      continue;
    }
    if (part.startsWith('[') && part.endsWith(']')) {
      const inner = part.slice(1, -1);
      const eq = inner.indexOf('=');
      out.push(':' + (eq === -1 ? inner : inner.slice(0, eq)));
      continue;
    }
    out.push(part);
  }

  return out.length === 0 ? '/' : '/' + out.join('/');
}
