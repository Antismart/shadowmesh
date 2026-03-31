import * as fs from 'fs';
import * as path from 'path';

export interface ScannedRoute {
  path: string;
  type: 'page' | 'api' | 'middleware';
  filePath: string;
  isDynamic: boolean;
}

const ROUTE_EXTENSIONS = ['.ts', '.tsx', '.js', '.jsx'];

export async function scanRoutes(projectDir: string): Promise<ScannedRoute[]> {
  const routes: ScannedRoute[] = [];

  // Scan App Router (app/ directory)
  const appDir = path.join(projectDir, 'app');
  if (fs.existsSync(appDir)) {
    await scanAppRouter(appDir, '', routes);
  }

  // Scan Pages Router (pages/api/ directory)
  const pagesApiDir = path.join(projectDir, 'pages', 'api');
  if (fs.existsSync(pagesApiDir)) {
    await scanPagesApi(pagesApiDir, '/api', routes);
  }

  return routes;
}

async function scanAppRouter(dir: string, urlPrefix: string, routes: ScannedRoute[]): Promise<void> {
  const entries = await fs.promises.readdir(dir, { withFileTypes: true });

  for (const entry of entries) {
    if (entry.name.startsWith('_') || entry.name.startsWith('.')) continue;

    const fullPath = path.join(dir, entry.name);

    if (entry.isDirectory()) {
      // Convert directory name to URL segment
      const segment = dirToSegment(entry.name);
      await scanAppRouter(fullPath, `${urlPrefix}/${segment}`, routes);
      continue;
    }

    const baseName = path.parse(entry.name).name;
    const ext = path.parse(entry.name).ext;
    if (!ROUTE_EXTENSIONS.includes(ext)) continue;

    const routePath = urlPrefix || '/';
    const isDynamic = routePath.includes(':');

    if (baseName === 'route') {
      // API route handler
      routes.push({
        path: routePath,
        type: 'api',
        filePath: fullPath,
        isDynamic,
      });
    } else if (baseName === 'page') {
      routes.push({
        path: routePath,
        type: 'page',
        filePath: fullPath,
        isDynamic,
      });
    } else if (baseName === 'middleware') {
      routes.push({
        path: routePath,
        type: 'middleware',
        filePath: fullPath,
        isDynamic,
      });
    }
  }
}

async function scanPagesApi(dir: string, urlPrefix: string, routes: ScannedRoute[]): Promise<void> {
  const entries = await fs.promises.readdir(dir, { withFileTypes: true });

  for (const entry of entries) {
    if (entry.name.startsWith('_') || entry.name.startsWith('.')) continue;

    const fullPath = path.join(dir, entry.name);

    if (entry.isDirectory()) {
      const segment = dirToSegment(entry.name);
      await scanPagesApi(fullPath, `${urlPrefix}/${segment}`, routes);
      continue;
    }

    const ext = path.parse(entry.name).ext;
    if (!ROUTE_EXTENSIONS.includes(ext)) continue;

    const baseName = path.parse(entry.name).name;
    const routePath = baseName === 'index'
      ? urlPrefix
      : `${urlPrefix}/${dirToSegment(baseName)}`;

    routes.push({
      path: routePath,
      type: 'api',
      filePath: fullPath,
      isDynamic: routePath.includes(':'),
    });
  }
}

/** Convert Next.js directory naming to URL segments.
 *  [id] → :id, [...slug] → *, (group) → stripped */
function dirToSegment(name: string): string {
  // Route groups — (marketing), (auth) — stripped from URL
  if (name.startsWith('(') && name.endsWith(')')) return '';

  // Catch-all — [...slug]
  if (name.startsWith('[...') && name.endsWith(']')) return '*';

  // Dynamic segment — [id]
  if (name.startsWith('[') && name.endsWith(']')) {
    return ':' + name.slice(1, -1);
  }

  return name;
}
