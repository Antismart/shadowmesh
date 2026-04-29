# @shadowmesh/adapter-sveltekit

SvelteKit adapter for [ShadowMesh](https://github.com/Antismart/shadowmesh) — a privacy-first decentralized CDN with WASM edge functions.

It plugs into `svelte.config.js` exactly like `@sveltejs/adapter-node`, scans your `src/routes/**` tree, and emits `_shadowmesh/routes.json` matching the gateway's manifest schema.

## Install

```bash
npm install -D @shadowmesh/adapter-sveltekit
```

Peer dependency: `@sveltejs/kit ^2.0`.

## Usage in `svelte.config.js`

```js
import adapter from '@shadowmesh/adapter-sveltekit';

export default {
  kit: {
    adapter: adapter({
      out: 'build',
      capabilities: {
        'api-users.wasm': ['net:connect'],
      },
    }),
  },
};
```

After `vite build`, the manifest is at `build/_shadowmesh/routes.json` and the static client + prerendered + server output is at `build/{client,prerendered,server}`.

## Options

| Option | Type | Default | Description |
| --- | --- | --- | --- |
| `out` | `string` | `"build"` | Output directory. |
| `staticPatterns` | `string[]` | `["/**"]` | Manifest `static` field. |
| `capabilities` | `Record<string, string[]>` | `undefined` | Manifest `capabilities` keyed by handler filename. |
| `skipKitOutput` | `boolean` | `false` | Skip writing `client/prerendered/server` if another adapter handles output. |
| `forceFilesystemScan` | `boolean` | `false` | Bypass `builder.routes` and scan `src/routes` instead. |

## Standalone CLI

For projects that want to emit a manifest without wiring the Kit adapter:

```bash
npx shadowmesh-sveltekit         # scans current directory
npx shadowmesh-sveltekit ./app   # scans ./app
```

The CLI requires a `svelte.config.{js,mjs,ts}` to be present and writes `<project>/_shadowmesh/routes.json`. Exits non-zero on error.

## Routing conventions honored

| SvelteKit pattern | Manifest path |
| --- | --- |
| `src/routes/+server.ts` | `/` |
| `src/routes/api/users/+server.ts` | `/api/users` |
| `src/routes/api/users/[id]/+server.ts` | `/api/users/:id` |
| `src/routes/blog/[...slug]/+server.ts` | `/blog/*` |
| `src/routes/(marketing)/about/+page.svelte` | `/about` |
| `src/routes/[id=integer]/+server.ts` | `/:id` (matcher dropped) |
| `src/routes/[[optional]]/+server.ts` | `/:optional` |

Files prefixed with `_` or `.` are skipped.

## Method detection

For `+server.{ts,js}` we **regex-scan** named exports for `GET`, `POST`, `PUT`, `DELETE`, `PATCH`, `HEAD`, `OPTIONS`. Both direct exports (`export const GET = ...`, `export async function POST() {}`) and brace re-exports (`export { handleGet as GET }`) are detected.

For `+page.server.{ts,js}` an exported `load` adds `GET` and an exported `actions` adds `POST`.

If no methods are detected the entry defaults to `["GET"]`.

**Limitations of the regex-based detector**:

- Computed export names are not recognized.
- Re-exports indirected through `export *` are not followed.
- TypeScript-only constructs (`declare const GET`) are skipped.

This is intentional — a full TS parse is out of scope. If you need exact detection, use `methodOverrides` (programmatic API) or open an issue with your case.

## Manifest output

```json
{
  "version": 1,
  "routes": [
    { "path": "/api/users/:id", "handler": "api-users-id.wasm", "methods": ["GET","POST"] }
  ],
  "static": ["/**"]
}
```

Handler names are sanitized to bare filenames (no `/`, no `\`, no `..`) so the gateway's `parse_manifest` accepts them. Empty/`.`/`..` handler names collapse to `index`.

## License

MIT
