/**
 * ShadowMesh SDK
 * Decentralized content hosting and delivery network
 */
export * from './types.js';
export { GatewayClient, NodeClient, createGatewayClient, createNodeClient, } from './client.js';
export { hashContent, hashString, deriveKey, encrypt, decrypt, serializeEncrypted, deserializeEncrypted, verifyHash, generateId, secureCompare, encryptToBase64, decryptFromBase64, randomBytes, type EncryptedData, } from './crypto.js';
export { type StorageBackend, MemoryStorage, IndexedDBStorage, type ContentStorageConfig, type StoredContent, ContentStorage, reassembleContent, fragmentContent, type IPFSConfig, IPFSClient, } from './storage.js';
export { type CacheEntry, type CacheStats, LRUCache, ContentCache, RequestDeduplicator, type FetchFunction, createCachedFetch, ContentPreloader, } from './cache.js';
export { formatBytes, parseBytes, formatDuration, formatRelativeTime, buildUrl, parseQuery, joinPaths, isValidCid, extractCid, buildIpfsUrl, type RetryOptions, retry, sleep, timeout, concurrent, debounce, throttle, deepClone, deepMerge, bytesToHex, hexToBytes, bytesToBase64, base64ToBytes, stringToBytes, bytesToString, assert, ensureDefined, isError, isBrowser, isNode, isWebWorker, EventEmitter, } from './utils.js';
export interface LegacyDeployOptions {
    path: string;
    domain?: string;
    ens?: string;
    privacy?: 'low' | 'medium' | 'high';
    redundancy?: number;
}
export interface LegacyDeploymentResult {
    gateway: string;
    native: string;
    cid: string;
    ens?: string;
    manifest: LegacyContentManifest;
}
export interface LegacyContentManifest {
    content_hash: string;
    fragments: string[];
    metadata: {
        name: string;
        size: number;
        mime_type: string;
    };
}
/**
 * @deprecated Use ShadowMeshClient instead
 */
export declare class ShadowMesh {
    private client;
    private networkEndpoint;
    constructor(options?: {
        network?: 'testnet' | 'mainnet';
    });
    deploy(options: LegacyDeployOptions): Promise<LegacyDeploymentResult>;
    private prepareContent;
    private fragmentContent;
    private uploadToStorage;
    private announceToNetwork;
    private registerENS;
    /**
     * Check the status of a deployment
     */
    status(cid: string): Promise<{
        available: boolean;
        replicas: number;
    }>;
    /**
     * Get network statistics
     */
    networkStats(): Promise<{
        nodes: number;
        bandwidth: number;
        files: number;
    }>;
}
export default ShadowMesh;
//# sourceMappingURL=index.d.ts.map