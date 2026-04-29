import * as fs from 'fs';
import * as path from 'path';
import { scanRoutes, type RouteConvention } from './route-scanner';
import { generateManifest, type ManifestOptions } from './manifest-generator';

/** Minimal Vite plugin shape — typed locally so we don't add a `vite` peer dep. */
export interface VitePluginShape {
  name: string;
  apply?: 'build' | 'serve';
  closeBundle: () => Promise<void> | void;
}

export interface ShadowMeshVitePluginOptions extends ManifestOptions {
  /** Project root; defaults to `process.cwd()`. */
  projectDir?: string;
  /**
   * Where the manifest is written, relative to `projectDir`. Defaults to
   * `build/client/_shadowmesh/routes.json` (Remix v2 client output dir).
   */
  outDir?: string;
  /** Force routing convention; auto-detected when omitted. */
  convention?: RouteConvention;
  /** Suppress console output. */
  quiet?: boolean;
}

export function shadowmeshRemix(options: ShadowMeshVitePluginOptions = {}): VitePluginShape {
  return {
    name: 'shadowmesh-remix',
    apply: 'build',
    async closeBundle() {
      const projectDir = options.projectDir ?? process.cwd();
      const routes = await scanRoutes(projectDir, { convention: options.convention });
      const manifest = await generateManifest(routes, {
        staticPatterns: options.staticPatterns,
        capabilities: options.capabilities,
        handlerExtension: options.handlerExtension,
      });

      const outDir = options.outDir
        ? path.resolve(projectDir, options.outDir)
        : path.join(projectDir, 'build', 'client', '_shadowmesh');
      await fs.promises.mkdir(outDir, { recursive: true });
      const outFile = path.join(outDir, 'routes.json');
      await fs.promises.writeFile(outFile, JSON.stringify(manifest, null, 2));

      if (!options.quiet) {
        // eslint-disable-next-line no-console
        console.log(
          `[shadowmesh-remix] wrote ${manifest.routes.length} route(s) → ${path.relative(projectDir, outFile)}`
        );
      }
    },
  };
}
