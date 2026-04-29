import adapter from './adapter';

export default adapter;
export { adapter };
export type { ShadowMeshAdapterOptions } from './adapter';
export { scanRoutes, dirToSegment } from './route-scanner';
export type { ScannedRoute, RouteType, ScanOptions } from './route-scanner';
export { generateManifest, routeToHandlerName } from './manifest-generator';
export type {
  RouteEntry,
  RouteManifest,
  ManifestOptions,
} from './manifest-generator';
export {
  detectMethods,
  detectMethodsFromSource,
  detectPageServerMethods,
  detectPageServerMethodsFromSource,
} from './method-detector';
export type { HttpMethod } from './method-detector';
