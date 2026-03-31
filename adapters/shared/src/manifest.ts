import * as fs from 'fs';
import * as path from 'path';

export interface RouteEntry {
  path: string;
  handler: string;
  methods?: string[];
}

export interface RouteManifest {
  version: 1;
  routes: RouteEntry[];
  static?: string[];
  capabilities?: Record<string, string[]>;
}

export class ManifestBuilder {
  private routes: RouteEntry[] = [];
  private staticPatterns: string[] = ['/**'];
  private capabilities: Record<string, string[]> = {};

  addRoute(routePath: string, handler: string, methods?: string[]): this {
    if (!handler || handler.includes('/') || handler.includes('..')) {
      throw new Error(`Invalid handler name: ${handler}`);
    }
    this.routes.push({ path: routePath, handler, methods });
    return this;
  }

  addCapability(handler: string, capability: string): this {
    if (!this.capabilities[handler]) {
      this.capabilities[handler] = [];
    }
    this.capabilities[handler].push(capability);
    return this;
  }

  setStaticPatterns(patterns: string[]): this {
    this.staticPatterns = patterns;
    return this;
  }

  build(): RouteManifest {
    return {
      version: 1,
      routes: this.routes,
      static: this.staticPatterns,
      capabilities: Object.keys(this.capabilities).length > 0 ? this.capabilities : undefined,
    };
  }

  toJSON(): string {
    return JSON.stringify(this.build(), null, 2);
  }

  async writeToDir(dir: string): Promise<void> {
    const smDir = path.join(dir, '_shadowmesh');
    await fs.promises.mkdir(smDir, { recursive: true });
    await fs.promises.writeFile(
      path.join(smDir, 'routes.json'),
      this.toJSON(),
      'utf-8'
    );
  }
}
