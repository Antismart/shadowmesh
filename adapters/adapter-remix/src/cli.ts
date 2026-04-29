#!/usr/bin/env node

import * as fs from 'fs';
import * as path from 'path';
import { scanRoutes, detectConvention } from './route-scanner';
import { generateManifest } from './manifest-generator';

async function main(): Promise<void> {
  const projectDir = path.resolve(process.argv[2] ?? '.');

  if (!fs.existsSync(projectDir) || !fs.statSync(projectDir).isDirectory()) {
    fail(`Project directory not found: ${projectDir}`);
  }

  const routesDir = path.join(projectDir, 'app', 'routes');
  const hasRoutesDir = fs.existsSync(routesDir);
  const hasRemixConfig = ['remix.config.js', 'remix.config.mjs', 'remix.config.cjs', 'remix.config.ts']
    .some((f) => fs.existsSync(path.join(projectDir, f)));
  const hasViteConfig = ['vite.config.ts', 'vite.config.js', 'vite.config.mjs']
    .some((f) => fs.existsSync(path.join(projectDir, f)));

  if (!hasRoutesDir && !hasRemixConfig && !hasViteConfig) {
    fail(
      `Not a Remix project: no app/routes/ and no remix.config.{js,ts}/vite.config.{ts,js} in ${projectDir}`
    );
  }

  const convention = hasRoutesDir
    ? await detectConvention(projectDir, routesDir)
    : 'v2-flat';

  console.log(`Scanning Remix routes (${convention})...`);
  const routes = await scanRoutes(projectDir, { convention });

  console.log(`Found ${routes.length} route(s):`);
  for (const r of routes) {
    const badge = r.isIndex ? '[INDEX]' : r.isSplat ? '[SPLAT]' : '[ROUTE]';
    const dyn = r.isDynamic ? ' (dynamic)' : '';
    console.log(`  ${badge} ${r.path}${dyn}`);
  }

  const manifest = await generateManifest(routes);

  const smDir = path.join(projectDir, '_shadowmesh');
  await fs.promises.mkdir(smDir, { recursive: true });
  const manifestPath = path.join(smDir, 'routes.json');
  await fs.promises.writeFile(manifestPath, JSON.stringify(manifest, null, 2));

  console.log(`\nManifest written to ${manifestPath}`);
  console.log(`  ${manifest.routes.length} route entry(ies)`);
}

function fail(msg: string): never {
  console.error(`Error: ${msg}`);
  process.exit(1);
}

main().catch((err: unknown) => {
  const message = err instanceof Error ? err.message : String(err);
  console.error('Error:', message);
  process.exit(1);
});
