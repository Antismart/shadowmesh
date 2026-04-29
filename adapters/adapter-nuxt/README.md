# @shadowmesh/adapter-nuxt

Nuxt 3 adapter for [ShadowMesh](https://github.com/Antismart/shadowmesh) — emits a `_shadowmesh/routes.json` manifest the gateway uses to route requests to WASM edge functions.

## Install

```bash
npm install --save-dev @shadowmesh/adapter-nuxt
```

## Use as a Nuxt module

```ts
// nuxt.config.ts
export default defineNuxtConfig({
  modules: ['@shadowmesh/adapter-nuxt'],
  shadowmesh: {
    edgeRoutes: ['/api/*'],
    staticPatterns: ['/**'],
    capabilities: {
      'api-users-id.wasm': ['net:connect'],
    },
  },
});
```

The module hooks `nitro:build:public-assets` and `build:done` to write `_shadowmesh/routes.json` into `.output/public/`.

## Use as a CLI

```bash
npx shadowmesh-nuxt           # scan current directory
npx shadowmesh-nuxt path/to/app
```

Writes the manifest to `<project>/_shadowmesh/routes.json`.

## Conventions honored

| Source                                  | Manifest path        |
| --------------------------------------- | -------------------- |
| `server/api/foo.ts`                     | `/api/foo`           |
| `server/api/users/[id].ts`              | `/api/users/:id`     |
| `server/api/posts/[...slug].ts`         | `/api/posts/*`       |
| `server/api/foo.get.ts` + `foo.post.ts` | `/api/foo` GET+POST  |
| `server/routes/health.ts`               | `/health`            |
| `server/middleware/auth.ts`             | (surfaced, not routed) |
| `server/api/(internal)/x.ts`            | `/api/x`             |

Files starting with `_` or `.` are skipped.

## Options

| Option           | Type                          | Default   |
| ---------------- | ----------------------------- | --------- |
| `edgeRoutes`     | `string[]`                    | all       |
| `staticPatterns` | `string[]`                    | `['/**']` |
| `capabilities`   | `Record<string, string[]>`    | none      |

## License

MIT
