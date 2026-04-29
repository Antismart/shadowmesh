import * as fs from 'fs';
import * as path from 'path';
import { scanRoutes } from './route-scanner';
import { generateManifest, type ManifestOptions } from './manifest-generator';

export interface ShadowMeshNuxtOptions extends ManifestOptions {}

interface NuxtHookable {
  hook(name: string, fn: (...args: unknown[]) => unknown | Promise<unknown>): void;
}

interface NuxtLike {
  options: {
    rootDir?: string;
    srcDir?: string;
    serverDir?: string;
  } & Record<string, unknown>;
  hook: NuxtHookable['hook'];
}

interface NitroLike {
  options: {
    output?: { publicDir?: string; dir?: string };
  } & Record<string, unknown>;
}

const MODULE_NAME = '@shadowmesh/adapter-nuxt';

async function writeManifest(
  projectDir: string,
  outputDir: string,
  options: ShadowMeshNuxtOptions
): Promise<string> {
  const routes = await scanRoutes(projectDir);
  const manifest = generateManifest(routes, options);
  const smDir = path.join(outputDir, '_shadowmesh');
  await fs.promises.mkdir(smDir, { recursive: true });
  const manifestPath = path.join(smDir, 'routes.json');
  await fs.promises.writeFile(
    manifestPath,
    JSON.stringify(manifest, null, 2)
  );
  return manifestPath;
}

function defineNuxtModuleFallback<T>(
  definition: {
    meta?: { name?: string; configKey?: string };
    defaults?: T;
    setup: (options: T, nuxt: NuxtLike) => void | Promise<void>;
  }
): (inlineOptions: Partial<T>, nuxt: NuxtLike) => Promise<void> {
  return async (inlineOptions, nuxt) => {
    const options = {
      ...(definition.defaults ?? ({} as T)),
      ...(inlineOptions ?? {}),
    } as T;
    await definition.setup(options, nuxt);
  };
}

function loadDefineNuxtModule(): typeof defineNuxtModuleFallback | null {
  try {
    const kit = require('@nuxt/kit') as {
      defineNuxtModule?: typeof defineNuxtModuleFallback;
    };
    return kit.defineNuxtModule ?? null;
  } catch {
    return null;
  }
}

const defineNuxtModule = loadDefineNuxtModule() ?? defineNuxtModuleFallback;

const shadowMeshNuxtModule = defineNuxtModule<ShadowMeshNuxtOptions>({
  meta: {
    name: MODULE_NAME,
    configKey: 'shadowmesh',
  },
  defaults: {},
  setup(options, nuxt) {
    const rootDir = nuxt.options.rootDir ?? process.cwd();

    nuxt.hook('nitro:build:public-assets', async (nitro: unknown) => {
      const n = nitro as NitroLike | undefined;
      const publicDir = n?.options?.output?.publicDir;
      if (!publicDir) return;
      try {
        await writeManifest(rootDir, publicDir, options);
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        console.warn(`[${MODULE_NAME}] manifest write skipped: ${msg}`);
      }
    });

    nuxt.hook('build:done', async () => {
      const outDir = path.join(rootDir, '.output', 'public');
      if (!fs.existsSync(outDir)) return;
      try {
        await writeManifest(rootDir, outDir, options);
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        console.warn(`[${MODULE_NAME}] manifest write skipped: ${msg}`);
      }
    });
  },
});

export default shadowMeshNuxtModule;
export { writeManifest };
