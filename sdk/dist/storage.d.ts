/**
 * ShadowMesh SDK - Storage Module
 * Client-side storage and IPFS integration
 */
import type { ContentManifest, StorageStats } from './types.js';
export interface StorageBackend {
    get(key: string): Promise<Uint8Array | null>;
    set(key: string, value: Uint8Array): Promise<void>;
    delete(key: string): Promise<boolean>;
    has(key: string): Promise<boolean>;
    list(prefix?: string): Promise<string[]>;
    clear(): Promise<void>;
    stats(): Promise<{
        keys: number;
        bytes: number;
    }>;
}
/**
 * In-memory storage backend for testing and ephemeral use
 */
export declare class MemoryStorage implements StorageBackend {
    private store;
    get(key: string): Promise<Uint8Array | null>;
    set(key: string, value: Uint8Array): Promise<void>;
    delete(key: string): Promise<boolean>;
    has(key: string): Promise<boolean>;
    list(prefix?: string): Promise<string[]>;
    clear(): Promise<void>;
    stats(): Promise<{
        keys: number;
        bytes: number;
    }>;
}
/**
 * IndexedDB storage backend for browser persistence
 */
export declare class IndexedDBStorage implements StorageBackend {
    private dbName;
    private storeName;
    private db;
    constructor(dbName?: string, storeName?: string);
    private getDB;
    get(key: string): Promise<Uint8Array | null>;
    set(key: string, value: Uint8Array): Promise<void>;
    delete(key: string): Promise<boolean>;
    has(key: string): Promise<boolean>;
    list(_prefix?: string): Promise<string[]>;
    clear(): Promise<void>;
    stats(): Promise<{
        keys: number;
        bytes: number;
    }>;
    close(): void;
}
export interface ContentStorageConfig {
    backend: StorageBackend;
    maxSize?: number;
    gcThreshold?: number;
}
export interface StoredContent {
    manifest: ContentManifest;
    fragments: Map<string, Uint8Array>;
    storedAt: Date;
    lastAccessed: Date;
    pinned: boolean;
}
/**
 * High-level content storage manager
 */
export declare class ContentStorage {
    private backend;
    private maxSize;
    private gcThreshold;
    private manifests;
    constructor(config: ContentStorageConfig);
    /**
     * Store content with its manifest
     */
    storeContent(cid: string, manifest: ContentManifest, fragments: Map<string, Uint8Array>): Promise<void>;
    /**
     * Retrieve content by CID
     */
    getContent(cid: string): Promise<{
        manifest: ContentManifest;
        fragments: Map<string, Uint8Array>;
    } | null>;
    /**
     * Get manifest only (without fragments)
     */
    getManifest(cid: string): Promise<ContentManifest | null>;
    /**
     * Get a specific fragment
     */
    getFragment(cid: string, fragmentId: string): Promise<Uint8Array | null>;
    /**
     * Store a single fragment
     */
    storeFragment(cid: string, fragmentId: string, data: Uint8Array): Promise<void>;
    /**
     * Check if content exists
     */
    hasContent(cid: string): Promise<boolean>;
    /**
     * Check if a specific fragment exists
     */
    hasFragment(cid: string, fragmentId: string): Promise<boolean>;
    /**
     * Delete content and all its fragments
     */
    deleteContent(cid: string): Promise<boolean>;
    /**
     * Pin content to prevent GC
     */
    pinContent(cid: string): Promise<void>;
    /**
     * Unpin content to allow GC
     */
    unpinContent(cid: string): Promise<void>;
    /**
     * List all stored content CIDs
     */
    listContent(): Promise<string[]>;
    /**
     * Get storage statistics
     */
    getStats(): Promise<StorageStats>;
    /**
     * Run garbage collection
     */
    runGC(targetBytes?: number): Promise<{
        freed: number;
        deleted: string[];
    }>;
    private maybeRunGC;
    private getMetadata;
    private setMetadata;
    private updateLastAccessed;
}
/**
 * Reassemble content from fragments
 */
export declare function reassembleContent(manifest: ContentManifest, fragments: Map<string, Uint8Array>): Uint8Array;
/**
 * Split content into fragments
 */
export declare function fragmentContent(data: Uint8Array, fragmentSize?: number): Array<{
    index: number;
    data: Uint8Array;
}>;
export interface IPFSConfig {
    gateway: string;
    apiUrl?: string;
    timeout?: number;
}
/**
 * IPFS client for content retrieval
 */
export declare class IPFSClient {
    private gateway;
    private apiUrl;
    private timeout;
    constructor(config: IPFSConfig);
    /**
     * Fetch content from IPFS gateway
     */
    get(cid: string): Promise<Uint8Array>;
    /**
     * Check if content exists on IPFS
     */
    exists(cid: string): Promise<boolean>;
    /**
     * Add content to IPFS (requires API access)
     */
    add(data: Uint8Array): Promise<string>;
    /**
     * Pin content on IPFS (requires API access)
     */
    pin(cid: string): Promise<void>;
    /**
     * Unpin content from IPFS (requires API access)
     */
    unpin(cid: string): Promise<void>;
}
//# sourceMappingURL=storage.d.ts.map