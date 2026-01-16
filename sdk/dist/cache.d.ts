/**
 * ShadowMesh SDK - Cache Module
 * Client-side caching for content and metadata
 */
import type { ContentManifest, CacheConfig } from './types.js';
export interface CacheEntry<T> {
    value: T;
    expiresAt: number;
    size: number;
    accessCount: number;
    lastAccessed: number;
}
export interface CacheStats {
    hits: number;
    misses: number;
    entries: number;
    bytes: number;
    hitRate: number;
}
/**
 * Generic LRU (Least Recently Used) cache
 */
export declare class LRUCache<T> {
    private cache;
    private maxSize;
    private defaultTTL;
    private currentSize;
    private hits;
    private misses;
    constructor(config?: CacheConfig);
    /**
     * Get a value from the cache
     */
    get(key: string): T | null;
    /**
     * Set a value in the cache
     */
    set(key: string, value: T, options?: {
        ttl?: number;
        size?: number;
    }): void;
    /**
     * Check if key exists and is not expired
     */
    has(key: string): boolean;
    /**
     * Delete a key from the cache
     */
    delete(key: string): boolean;
    /**
     * Clear all entries
     */
    clear(): void;
    /**
     * Get cache statistics
     */
    stats(): CacheStats;
    /**
     * Get all keys
     */
    keys(): string[];
    /**
     * Cleanup expired entries
     */
    cleanup(): number;
    private evictLRU;
    private estimateSize;
}
/**
 * Specialized cache for ShadowMesh content
 */
export declare class ContentCache {
    private manifestCache;
    private fragmentCache;
    private responseCache;
    constructor(config?: CacheConfig);
    getManifest(cid: string): ContentManifest | null;
    setManifest(cid: string, manifest: ContentManifest, ttl?: number): void;
    hasManifest(cid: string): boolean;
    getFragment(cid: string, fragmentId: string): Uint8Array | null;
    setFragment(cid: string, fragmentId: string, data: Uint8Array, ttl?: number): void;
    hasFragment(cid: string, fragmentId: string): boolean;
    getResponse(url: string): Response | null;
    setResponse(url: string, response: Response, ttl?: number): void;
    clear(): void;
    cleanup(): {
        manifests: number;
        fragments: number;
        responses: number;
    };
    stats(): {
        manifests: CacheStats;
        fragments: CacheStats;
        responses: CacheStats;
        total: CacheStats;
    };
}
/**
 * Deduplicates concurrent requests to the same resource
 */
export declare class RequestDeduplicator {
    private pending;
    private timeout;
    constructor(timeout?: number);
    /**
     * Deduplicate a request - if same request is pending, return existing promise
     */
    dedupe<T>(key: string, factory: () => Promise<T>): Promise<T>;
    /**
     * Cancel a pending request
     */
    cancel(key: string): boolean;
    /**
     * Get count of pending requests
     */
    get pendingCount(): number;
    private cleanup;
}
export type FetchFunction = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;
/**
 * Create a fetch wrapper with caching
 */
export declare function createCachedFetch(cache: ContentCache, options?: {
    ttl?: number;
    cacheableStatus?: number[];
    cacheableMethods?: string[];
}): FetchFunction;
/**
 * Preloads content for faster access
 */
export declare class ContentPreloader {
    private cache;
    private deduplicator;
    private fetchFn;
    constructor(cache: ContentCache, options?: {
        fetchFn?: typeof fetch;
    });
    /**
     * Preload content manifest
     */
    preloadManifest(gatewayUrl: string, cid: string): Promise<ContentManifest>;
    /**
     * Preload content fragments
     */
    preloadFragments(gatewayUrl: string, cid: string, fragmentIds?: string[], concurrency?: number): Promise<Map<string, Uint8Array>>;
    /**
     * Preload related content (e.g., linked resources)
     */
    preloadRelated(gatewayUrl: string, cids: string[], manifestsOnly?: boolean): Promise<void>;
}
//# sourceMappingURL=cache.d.ts.map