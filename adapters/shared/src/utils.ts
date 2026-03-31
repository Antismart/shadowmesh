import * as fs from 'fs';
import * as path from 'path';

export async function detectFramework(dir: string): Promise<string> {
  const exists = (f: string) => fs.existsSync(path.join(dir, f));

  if (exists('next.config.js') || exists('next.config.ts') || exists('next.config.mjs')) return 'nextjs';
  if (exists('nuxt.config.ts') || exists('nuxt.config.js')) return 'nuxt';
  if (exists('svelte.config.js') || exists('svelte.config.ts')) return 'sveltekit';
  if (exists('remix.config.js') || exists('remix.config.ts')) return 'remix';
  if (exists('astro.config.mjs') || exists('astro.config.ts')) return 'astro';
  if (exists('vite.config.ts') || exists('vite.config.js')) return 'vite';
  if (exists('gatsby-config.js') || exists('gatsby-config.ts')) return 'gatsby';
  if (exists('angular.json')) return 'angular';
  if (exists('package.json')) return 'node';
  if (exists('index.html')) return 'static';
  return 'unknown';
}

export function findOutputDir(dir: string, framework: string): string | null {
  const candidates: Record<string, string[]> = {
    nextjs: ['out', '.next/standalone'],
    nuxt: ['.output/public', '.output'],
    sveltekit: ['build'],
    remix: ['build'],
    astro: ['dist'],
    vite: ['dist'],
    gatsby: ['public'],
    angular: ['dist'],
    node: ['dist', 'build'],
    static: ['.'],
  };

  const dirs = candidates[framework] || ['dist', 'build', 'out', 'public'];
  for (const d of dirs) {
    const full = path.join(dir, d);
    if (fs.existsSync(full) && fs.statSync(full).isDirectory()) {
      return full;
    }
  }
  return null;
}

export async function ensureShadowMeshDir(dir: string): Promise<string> {
  const smDir = path.join(dir, '_shadowmesh');
  await fs.promises.mkdir(smDir, { recursive: true });
  return smDir;
}

export async function copyWasmModule(src: string, destDir: string, name: string): Promise<void> {
  const smDir = await ensureShadowMeshDir(destDir);
  const dest = path.join(smDir, name);
  await fs.promises.copyFile(src, dest);
}
