export {
  scanRoutes,
  detectConvention,
  v2FilenameToPath,
  type ScannedRoute,
  type ScanOptions,
  type RouteConvention,
} from './route-scanner';

export {
  generateManifest,
  routeToHandlerStem,
  type RouteManifest,
  type RouteManifestEntry,
  type ManifestOptions,
} from './manifest-generator';

export {
  detectMethods,
  detectMethodsFromSource,
  type HttpMethod,
  type DetectedMethods,
} from './method-detector';

export {
  withShadowMesh,
  type ShadowMeshRemixConfig,
  type ShadowMeshAnnotation,
} from './adapter';

export {
  shadowmeshRemix,
  type ShadowMeshVitePluginOptions,
  type VitePluginShape,
} from './vite-plugin';
