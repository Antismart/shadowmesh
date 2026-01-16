/**
 * ShadowMesh SDK Types
 * 
 * Comprehensive TypeScript types for the ShadowMesh SDK.
 */

// ============================================================================
// Configuration Types
// ============================================================================

/**
 * SDK configuration options
 */
export interface ShadowMeshConfig {
  /** Network to connect to */
  network?: 'testnet' | 'mainnet' | 'local';
  /** Custom gateway URL */
  gatewayUrl?: string;
  /** Custom node URL for direct P2P */
  nodeUrl?: string;
  /** API key for authenticated requests */
  apiKey?: string;
  /** Request timeout in milliseconds */
  timeout?: number;
  /** Enable debug logging */
  debug?: boolean;
  /** Cache configuration */
  cache?: CacheConfig;
}

/**
 * Cache configuration
 */
export interface CacheConfig {
  /** Enable client-side caching */
  enabled?: boolean;
  /** Maximum cache size in bytes */
  maxSize?: number;
  /** Default TTL in seconds */
  ttl?: number;
  /** Storage backend */
  storage?: 'memory' | 'filesystem' | 'indexeddb';
}

// ============================================================================
// Content Types
// ============================================================================

/**
 * Content manifest describing fragmented content
 */
export interface ContentManifest {
  /** Blake3 hash of complete content */
  contentHash: string;
  /** Array of fragment hashes */
  fragments: FragmentInfo[];
  /** Content metadata */
  metadata: ContentMetadata;
  /** Manifest version */
  version: number;
  /** Creation timestamp */
  createdAt: number;
  /** Total size in bytes */
  size: number;
}

/**
 * Fragment information in manifest
 */
export interface FragmentInfo {
  /** Fragment ID/hash */
  id: string;
  /** Fragment index */
  index: number;
  /** Fragment size in bytes */
  size: number;
  /** Optional node locations */
  nodes?: string[];
}

/**
 * Content metadata
 */
export interface ContentMetadata {
  /** Original filename */
  name: string;
  /** Size in bytes */
  size: number;
  /** MIME type */
  mimeType: string;
  /** Optional description */
  description?: string;
  /** Optional tags for discovery */
  tags?: string[];
  /** Whether content is encrypted */
  encrypted?: boolean;
  /** Encryption algorithm used */
  encryptionAlgorithm?: string;
}

/**
 * Content fragment
 */
export interface ContentFragment {
  /** Fragment index */
  index: number;
  /** Total number of fragments */
  totalFragments: number;
  /** Fragment data */
  data: Uint8Array;
  /** Fragment hash */
  hash: string;
}

/**
 * Content upload options
 */
export interface UploadOptions {
  /** Original filename */
  name?: string;
  /** MIME type (auto-detected if not provided) */
  mimeType?: string;
  /** Enable encryption */
  encrypt?: boolean;
  /** Encryption password (required if encrypt is true) */
  password?: string;
  /** Privacy level */
  privacy?: PrivacyLevel;
  /** Number of replicas */
  redundancy?: number;
  /** Optional tags */
  tags?: string[];
  /** Progress callback */
  onProgress?: (progress: UploadProgress) => void;
}

/**
 * Privacy levels
 */
export type PrivacyLevel = 'low' | 'medium' | 'high';

/**
 * Upload progress information
 */
export interface UploadProgress {
  /** Current phase */
  phase: 'preparing' | 'fragmenting' | 'encrypting' | 'uploading' | 'announcing';
  /** Bytes processed */
  bytesProcessed: number;
  /** Total bytes */
  totalBytes: number;
  /** Progress percentage (0-100) */
  percentage: number;
  /** Current fragment being processed */
  currentFragment?: number;
  /** Total fragments */
  totalFragments?: number;
}

/**
 * Download options
 */
export interface DownloadOptions {
  /** Decryption password */
  password?: string;
  /** Progress callback */
  onProgress?: (progress: DownloadProgress) => void;
  /** Verify content hash */
  verify?: boolean;
}

/**
 * Download progress information
 */
export interface DownloadProgress {
  /** Current phase */
  phase: 'fetching' | 'downloading' | 'decrypting' | 'reassembling' | 'verifying';
  /** Bytes downloaded */
  bytesDownloaded: number;
  /** Total bytes */
  totalBytes: number;
  /** Progress percentage (0-100) */
  percentage: number;
  /** Current fragment being downloaded */
  currentFragment?: number;
  /** Total fragments */
  totalFragments?: number;
}

// ============================================================================
// Deployment Types
// ============================================================================

/**
 * Deployment options
 */
export interface DeployOptions {
  /** Path to file or directory */
  path: string;
  /** Custom domain name */
  domain?: string;
  /** ENS domain name */
  ens?: string;
  /** Privacy level */
  privacy?: PrivacyLevel;
  /** Number of replicas */
  redundancy?: number;
  /** Enable encryption */
  encrypt?: boolean;
  /** Encryption password */
  password?: string;
  /** Pin content permanently */
  pin?: boolean;
}

/**
 * Deployment result
 */
export interface DeploymentResult {
  /** Gateway URL for HTTP access */
  gateway: string;
  /** Native shadow:// URL */
  native: string;
  /** Content ID (CID) */
  cid: string;
  /** ENS URL if registered */
  ens?: string;
  /** Content manifest */
  manifest: ContentManifest;
  /** Deployment timestamp */
  deployedAt: number;
  /** Transaction hash if applicable */
  txHash?: string;
}

/**
 * Deployment status
 */
export interface DeploymentStatus {
  /** Content ID */
  cid: string;
  /** Whether content is available */
  available: boolean;
  /** Number of active replicas */
  replicas: number;
  /** Geographic distribution */
  regions?: string[];
  /** Health status */
  health: 'healthy' | 'degraded' | 'unavailable';
  /** Last check timestamp */
  lastChecked: number;
}

// ============================================================================
// Network Types
// ============================================================================

/**
 * Network statistics
 */
export interface NetworkStats {
  /** Number of active nodes */
  nodes: number;
  /** Total bandwidth served (bytes) */
  bandwidth: number;
  /** Number of files hosted */
  files: number;
  /** Total storage used (bytes) */
  storage: number;
  /** Number of active requests */
  activeRequests: number;
  /** Average latency in ms */
  avgLatency: number;
}

/**
 * Storage statistics
 */
export interface StorageStats {
  /** Total storage capacity in bytes */
  totalBytes: number;
  /** Bytes currently used */
  usedBytes: number;
  /** Bytes available */
  availableBytes: number;
  /** Number of stored content items */
  contentCount: number;
  /** Number of stored fragments */
  fragmentCount: number;
}

/**
 * Node information
 */
export interface NodeInfo {
  /** Node peer ID */
  peerId: string;
  /** Node name */
  name: string;
  /** Node version */
  version: string;
  /** Node region */
  region?: string;
  /** Multiaddresses */
  addresses: string[];
  /** Node uptime in seconds */
  uptime: number;
  /** Connected peers count */
  connectedPeers: number;
}

/**
 * Peer information
 */
export interface PeerInfo {
  /** Peer ID */
  peerId: string;
  /** Multiaddresses */
  addresses: string[];
  /** Whether peer is connected */
  connected: boolean;
  /** Latency in ms */
  latency?: number;
  /** Peer protocols */
  protocols?: string[];
}

// ============================================================================
// Gateway Types
// ============================================================================

/**
 * Gateway configuration
 */
export interface GatewayConfig {
  /** Gateway URL */
  url: string;
  /** API key */
  apiKey?: string;
  /** Request timeout */
  timeout?: number;
}

/**
 * Gateway health status
 */
export interface GatewayHealth {
  /** Whether gateway is healthy */
  healthy: boolean;
  /** Gateway version */
  version: string;
  /** Uptime in seconds */
  uptime: number;
  /** Cache hit rate (0-100) */
  cacheHitRate: number;
  /** Active connections */
  activeConnections: number;
}

/**
 * Gateway metrics
 */
export interface GatewayMetrics {
  /** Total requests */
  totalRequests: number;
  /** Successful requests */
  successfulRequests: number;
  /** Failed requests */
  failedRequests: number;
  /** Cache hits */
  cacheHits: number;
  /** Cache misses */
  cacheMisses: number;
  /** Bytes served */
  bytesServed: number;
  /** Average response time in ms */
  avgResponseTime: number;
}

// ============================================================================
// Error Types
// ============================================================================

/**
 * ShadowMesh error codes
 */
export enum ErrorCode {
  // Network errors
  NETWORK_ERROR = 'NETWORK_ERROR',
  TIMEOUT = 'TIMEOUT',
  CONNECTION_REFUSED = 'CONNECTION_REFUSED',
  
  // Content errors
  CONTENT_NOT_FOUND = 'CONTENT_NOT_FOUND',
  CONTENT_UNAVAILABLE = 'CONTENT_UNAVAILABLE',
  INVALID_CID = 'INVALID_CID',
  HASH_MISMATCH = 'HASH_MISMATCH',
  
  // Encryption errors
  ENCRYPTION_FAILED = 'ENCRYPTION_FAILED',
  DECRYPTION_FAILED = 'DECRYPTION_FAILED',
  INVALID_PASSWORD = 'INVALID_PASSWORD',
  
  // Upload errors
  UPLOAD_FAILED = 'UPLOAD_FAILED',
  FRAGMENT_FAILED = 'FRAGMENT_FAILED',
  STORAGE_FULL = 'STORAGE_FULL',
  
  // API errors
  UNAUTHORIZED = 'UNAUTHORIZED',
  RATE_LIMITED = 'RATE_LIMITED',
  INVALID_REQUEST = 'INVALID_REQUEST',
  
  // Other
  UNKNOWN_ERROR = 'UNKNOWN_ERROR',
}

/**
 * ShadowMesh error
 */
export interface ShadowMeshError extends Error {
  /** Error code */
  code: ErrorCode;
  /** HTTP status code if applicable */
  statusCode?: number;
  /** Additional details */
  details?: Record<string, unknown>;
}

// ============================================================================
// Event Types
// ============================================================================

/**
 * SDK event types
 */
export type SDKEventType = 
  | 'upload:start'
  | 'upload:progress'
  | 'upload:complete'
  | 'upload:error'
  | 'download:start'
  | 'download:progress'
  | 'download:complete'
  | 'download:error'
  | 'connection:established'
  | 'connection:lost'
  | 'peer:connected'
  | 'peer:disconnected';

/**
 * SDK event
 */
export interface SDKEvent<T = unknown> {
  /** Event type */
  type: SDKEventType;
  /** Event timestamp */
  timestamp: number;
  /** Event data */
  data: T;
}

/**
 * Event listener
 */
export type EventListener<T = unknown> = (event: SDKEvent<T>) => void;

// ============================================================================
// Utility Types
// ============================================================================

/**
 * Paginated response
 */
export interface PaginatedResponse<T> {
  /** Data items */
  items: T[];
  /** Total count */
  total: number;
  /** Current page */
  page: number;
  /** Items per page */
  perPage: number;
  /** Whether there are more pages */
  hasMore: boolean;
}

/**
 * API response wrapper
 */
export interface ApiResponse<T> {
  /** Whether request was successful */
  success: boolean;
  /** Response data */
  data?: T;
  /** Error message if failed */
  error?: string;
  /** Error code if failed */
  errorCode?: ErrorCode;
}
