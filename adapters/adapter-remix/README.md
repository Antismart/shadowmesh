# @shadowmesh/adapter-remix

Remix adapter for [ShadowMesh](https://github.com/Antismart/shadowmesh) — a privacy-first
decentralized CDN. Generates a `_shadowmesh/routes.json` manifest from your Remix routes
that the ShadowMesh gateway uses to dispatch requests.

Supports both **Remix v2 flat-file** routing (`app/routes/users.$id.tsx`) and the
**v1 nested directory** convention (`app/routes/users/$id.tsx`). The convention is
auto-detected from your project layout, but can be forced.

## Install

```bash
npm install --save-dev @shadowmesh/adapter-remix
```

## Usage

### Remix v2 (Vite) — recommended

Add the plugin to `vite.config.ts`:

```ts
import { vitePlugin as remix } from '@remix-run/dev';
import { defineConfig } from 'vite';
import { shadowmeshRemix } from '@shadowmesh/adapter-remix';

export default defineConfig({
  plugins: [
    remix(),
    shadowmeshRemix({
      capabilities: { 'api-users-id.wasm': ['net:connect'] },
    }),
  ],
});
```

After `remix vite:build`, the manifest is written to `build/client/_shadowmesh/routes.json`.

### Remix v1 (CLI flow)

Remix v1 has no first-class build hook, so use the CLI after each build:

```bash
npx shadowmesh-remix .
```

The `withShadowMesh` helper for `remix.config.js` is a passthrough that annotates
the config with ShadowMesh options for tooling — it does not run during Remix's
build itself:

```js
const { withShadowMesh } = require('@shadowmesh/adapter-remix');
module.exports = withShadowMesh(
  { ignoredRouteFiles: ['**/.*'] },
  { capabilities: { 'api-post.wasm': ['net:connect'] } }
);
```

### CLI

```bash
npx shadowmesh-remix [project-dir]   # defaults to .
```

Writes `<project>/_shadowmesh/routes.json`. Exits non-zero on error.

## Routing rules

| Filename (v2)               | URL              |
| --------------------------- | ---------------- |
| `_index.tsx`                | `/`              |
| `about.tsx`                 | `/about`         |
| `users.$id.tsx`             | `/users/:id`     |
| `blog.$.tsx`                | `/blog/*`        |
| `users_.profile.tsx`        | `/users/profile` |
| `_marketing.about.tsx`      | `/about`         |
| `($lang).about.tsx`         | `/about`         |

**Optional segments** like `($lang)` are emitted only as the *non-optional* variant
(`/about`). If you need the parameterized form, declare it explicitly in a separate
file. This keeps the manifest small and unambiguous for the gateway matcher.

## Method detection

A single regex pass over each route module looks for these named exports:

- `loader` → `GET`
- `action` → `POST`, `PUT`, `PATCH`, `DELETE` (Remix dispatches all mutating methods to a single `action`)

A route with neither export is treated as UI-only and gets `["GET"]`. We don't run
a real TypeScript parser here — the trade-off is a small risk of false positives
on contrived comments, which inflates the method set harmlessly.

## Manifest schema

```json
{
  "version": 1,
  "routes": [
    { "path": "/users/:id", "handler": "users-id.wasm", "methods": ["GET"] },
    { "path": "/blog/*", "handler": "blog-splat.wasm", "methods": ["GET"] }
  ],
  "static": ["/**"],
  "capabilities": {}
}
```

Handler names are sanitized: `/users/:id` → `users-id.wasm`. Collisions are
disambiguated with a numeric suffix.
