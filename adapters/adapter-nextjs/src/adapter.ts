export interface ShadowMeshNextConfig {
  edgeRoutes?: string[];
  staticPatterns?: string[];
  capabilities?: Record<string, string[]>;
}

/**
 * Wraps a Next.js config with ShadowMesh edge function support.
 *
 * Usage in next.config.js:
 * ```js
 * const { withShadowMesh } = require('@shadowmesh/adapter-nextjs');
 *
 * module.exports = withShadowMesh({
 *   // your normal Next.js config
 * }, {
 *   edgeRoutes: ['/api/*'],
 * });
 * ```
 */
export function withShadowMesh(
  nextConfig: Record<string, any>,
  smConfig?: ShadowMeshNextConfig
): Record<string, any> {
  const originalWebpack = nextConfig.webpack;

  return {
    ...nextConfig,
    // Ensure standalone output for SSR deployments
    output: nextConfig.output || 'standalone',
    webpack: (config: any, context: any) => {
      // Call original webpack config if it exists
      if (typeof originalWebpack === 'function') {
        config = originalWebpack(config, context);
      }

      // Add ShadowMesh config as a define for runtime access
      if (context.isServer && smConfig) {
        config.plugins = config.plugins || [];
        // Store config for postbuild manifest generation
        const DefinePlugin = context.webpack?.DefinePlugin;
        if (DefinePlugin) {
          config.plugins.push(
            new DefinePlugin({
              'process.env.__SHADOWMESH_CONFIG': JSON.stringify(
                JSON.stringify(smConfig)
              ),
            })
          );
        }
      }

      return config;
    },
  };
}
