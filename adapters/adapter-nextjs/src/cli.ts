#!/usr/bin/env node

import * as path from 'path';
import * as fs from 'fs';
import { scanRoutes } from './route-scanner';
import { generateManifest } from './manifest-generator';

async function main() {
  const projectDir = path.resolve(process.argv[2] || '.');

  if (!fs.existsSync(path.join(projectDir, 'package.json'))) {
    console.error('Error: No package.json found in', projectDir);
    process.exit(1);
  }

  const isNextJs =
    fs.existsSync(path.join(projectDir, 'next.config.js')) ||
    fs.existsSync(path.join(projectDir, 'next.config.ts')) ||
    fs.existsSync(path.join(projectDir, 'next.config.mjs'));

  if (!isNextJs) {
    console.error('Error: Not a Next.js project (no next.config.{js,ts,mjs} found)');
    process.exit(1);
  }

  console.log('Scanning Next.js routes...');
  const routes = await scanRoutes(projectDir);

  console.log(`Found ${routes.length} routes:`);
  for (const route of routes) {
    const badge = route.type === 'api' ? '[API]' : route.type === 'page' ? '[PAGE]' : '[MW]';
    const dynamic = route.isDynamic ? ' (dynamic)' : '';
    console.log(`  ${badge} ${route.path}${dynamic}`);
  }

  const manifest = generateManifest(routes);

  const smDir = path.join(projectDir, '_shadowmesh');
  await fs.promises.mkdir(smDir, { recursive: true });
  const manifestPath = path.join(smDir, 'routes.json');
  await fs.promises.writeFile(manifestPath, JSON.stringify(manifest, null, 2));

  console.log(`\nManifest written to ${manifestPath}`);
  console.log(`  ${manifest.routes.length} edge function route(s)`);
}

main().catch((err) => {
  console.error('Error:', err.message);
  process.exit(1);
});
