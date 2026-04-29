#!/usr/bin/env node

import * as fs from 'fs';
import * as path from 'path';
import { scanRoutes, type ScannedRoute } from './route-scanner';
import { generateManifest } from './manifest-generator';

function findSvelteConfig(projectDir: string): string | null {
  const candidates = ['svelte.config.js', 'svelte.config.mjs', 'svelte.config.ts'];
  for (const c of candidates) {
    const p = path.join(projectDir, c);
    if (fs.existsSync(p)) return p;
  }
  return null;
}

function badge(route: ScannedRoute): string {
  switch (route.type) {
    case 'api':
      return '[API]';
    case 'page':
      return '[PAGE]';
    case 'page-server':
      return '[PAGE]';
    case 'layout':
      return '[LAYOUT]';
  }
}

async function main(): Promise<void> {
  const projectDir = path.resolve(process.argv[2] ?? '.');

  if (!fs.existsSync(path.join(projectDir, 'package.json'))) {
    console.error(`Error: No package.json found in ${projectDir}`);
    process.exit(1);
  }

  const svelteConfig = findSvelteConfig(projectDir);
  if (!svelteConfig) {
    console.error(
      'Error: Not a SvelteKit project (no svelte.config.{js,mjs,ts} found)'
    );
    process.exit(1);
  }

  console.log(`Scanning SvelteKit routes in ${path.relative(process.cwd(), projectDir) || '.'}...`);
  const routes = await scanRoutes(projectDir);

  console.log(`Found ${routes.length} route entr${routes.length === 1 ? 'y' : 'ies'}:`);
  for (const route of routes) {
    const dyn = route.isDynamic ? ' (dynamic)' : '';
    console.log(`  ${badge(route)} ${route.path}${dyn}`);
  }

  const manifest = await generateManifest(routes);

  const smDir = path.join(projectDir, '_shadowmesh');
  await fs.promises.mkdir(smDir, { recursive: true });
  const manifestPath = path.join(smDir, 'routes.json');
  await fs.promises.writeFile(manifestPath, JSON.stringify(manifest, null, 2));

  console.log(`\nManifest written to ${manifestPath}`);
  console.log(`  ${manifest.routes.length} edge function route(s)`);
}

main().catch((err: unknown) => {
  const message = err instanceof Error ? err.message : String(err);
  console.error('Error:', message);
  process.exit(1);
});
