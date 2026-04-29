export interface ShadowMeshRemixConfig {
  staticPatterns?: string[];
  capabilities?: Record<string, string[]>;
  /** Forwarded to the manifest generator (rarely needed). */
  handlerExtension?: string;
}

export interface ShadowMeshAnnotation {
  __shadowmesh: ShadowMeshRemixConfig;
}

/**
 * Wrap a Remix v1 `remix.config.js` export so build tooling can read the
 * ShadowMesh config off the resolved object. This is a passthrough — the
 * actual manifest is written by the CLI (`npx shadowmesh-remix`) since
 * Remix v1 has no first-class build hook we can latch onto without forking.
 *
 * Usage:
 *   const { withShadowMesh } = require('@shadowmesh/adapter-remix');
 *   module.exports = withShadowMesh({ ignoredRouteFiles: ['**\/.*'] }, {
 *     capabilities: { 'api-users-id.wasm': ['net:connect'] },
 *   });
 */
export function withShadowMesh<T extends Record<string, unknown>>(
  remixConfig: T,
  smConfig: ShadowMeshRemixConfig = {}
): T & ShadowMeshAnnotation {
  return {
    ...remixConfig,
    __shadowmesh: smConfig,
  };
}
