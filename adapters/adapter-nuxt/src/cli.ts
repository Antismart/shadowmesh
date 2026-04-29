#!/usr/bin/env node

import * as fs from 'fs';
import * as path from 'path';
import { scanRoutes } from './route-scanner';
import { generateManifest } from './manifest-generator';

const NUXT_CONFIG_NAMES = [
  'nuxt.config.ts',
  'nuxt.config.js',
  'nuxt.config.mjs',
  'nuxt.config.cjs',
  'nuxt.config.mts',
];

async function main(): Promise<void> {
  const projectDir = path.resolve(process.argv[2] ?? '.');

  if (!fs.existsSync(projectDir) || !fs.statSync(projectDir).isDirectory()) {
    console.error(`Error: ${projectDir} is not a directory`);
    process.exit(1);
  }

  if (!fs.existsSync(path.join(projectDir, 'package.json'))) {
    console.error(`Error: No package.json found in ${projectDir}`);
    process.exit(1);
  }

  const isNuxt = NUXT_CONFIG_NAMES.some((name) =>
    fs.existsSync(path.join(projectDir, name))
  );

  if (!isNuxt) {
    console.error(
      `Error: Not a Nuxt project (no ${NUXT_CONFIG_NAMES.join(', ')} found in ${projectDir})`
    );
    process.exit(1);
  }

  console.log('Scanning Nuxt server routes...');
  const routes = await scanRoutes(projectDir);

  console.log(`Found ${routes.length} entr${routes.length === 1 ? 'y' : 'ies'}:`);
  for (const route of routes) {
    const badge =
      route.type === 'api'
        ? '[API]'
        : route.type === 'route'
          ? '[ROUTE]'
          : '[MW]';
    const methods =
      route.type === 'middleware'
        ? ''
        : route.methods && route.methods.length > 0
          ? ` ${route.methods.join(',')}`
          : ' *';
    const dynamic = route.isDynamic ? ' (dynamic)' : '';
    console.log(`  ${badge} ${route.path}${methods}${dynamic}`);
  }

  const manifest = generateManifest(routes);

  const smDir = path.join(projectDir, '_shadowmesh');
  await fs.promises.mkdir(smDir, { recursive: true });
  const manifestPath = path.join(smDir, 'routes.json');
  await fs.promises.writeFile(
    manifestPath,
    JSON.stringify(manifest, null, 2)
  );

  console.log(`\nManifest written to ${manifestPath}`);
  console.log(`  ${manifest.routes.length} edge function route(s)`);
}

main().catch((err: unknown) => {
  const msg = err instanceof Error ? err.message : String(err);
  console.error('Error:', msg);
  process.exit(1);
});
