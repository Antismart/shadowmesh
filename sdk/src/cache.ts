/**
 * ShadowMesh SDK - Cache Module
 * Client-side caching for content and metadata
 */

import type {
  ContentManifest,
  CacheConfig,
} from './types.js';

// ============================================================================
// Cache Interfaces
// ============================================================================

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

// ============================================================================
// LRU Cache Implementation
// ============================================================================

/**
 * Generic LRU (Least Recently Used) cache
 */
export class LRUCache<T> {
  private cache: Map<string, CacheEntry<T>> = new Map();
  private maxSize: number;
  private defaultTTL: number;
  private currentSize: number = 0;
  private hits: number = 0;
  private misses: number = 0;

  constructor(config: CacheConfig = {}) {
    this.maxSize = config.maxSize ?? 50 * 1024 * 1024; // 50MB default
    this.defaultTTL = (config.ttl ?? 300) * 1000; // Convert to ms
  }

  /**
   * Get a value from the cache
   */
  get(key: string): T | null {
    const entry = this.cache.get(key);

    if (!entry) {
      this.misses++;
      return null;
    }

    // Check expiration
    if (Date.now() > entry.expiresAt) {
      this.delete(key);
      this.misses++;
      return null;
    }

    // Update access stats
    entry.accessCount++;
    entry.lastAccessed = Date.now();
    this.hits++;

    // Move to end (most recently used)
    this.cache.delete(key);
    this.cache.set(key, entry);

    return entry.value;
  }

  /**
   * Set a value in the cache
   */
  set(key: string, value: T, options?: { ttl?: number; size?: number }): void {
    const size = options?.size ?? this.estimateSize(value);
    const ttl = options?.ttl ?? this.defaultTTL;

    // Remove existing entry if present
    if (this.cache.has(key)) {
      this.delete(key);
    }

    // Evict entries if needed
    while (this.currentSize + size > this.maxSize && this.cache.size > 0) {
      this.evictLRU();
    }

    const entry: CacheEntry<T> = {
      value,
      expiresAt: Date.now() + ttl,
      size,
      accessCount: 1,
      lastAccessed: Date.now(),
    };

    this.cache.set(key, entry);
    this.currentSize += size;
  }

  /**
   * Check if key exists and is not expired
   */
  has(key: string): boolean {
    const entry = this.cache.get(key);
    if (!entry) return false;
    if (Date.now() > entry.expiresAt) {
      this.delete(key);
      return false;
    }
    return true;
  }

  /**
   * Delete a key from the cache
   */
  delete(key: string): boolean {
    const entry = this.cache.get(key);
    if (!entry) return false;

    this.currentSize -= entry.size;
    return this.cache.delete(key);
  }

  /**
   * Clear all entries
   */
  clear(): void {
    this.cache.clear();
    this.currentSize = 0;
  }

  /**
   * Get cache statistics
   */
  stats(): CacheStats {
    const total = this.hits + this.misses;
    return {
      hits: this.hits,
      misses: this.misses,
      entries: this.cache.size,
      bytes: this.currentSize,
      hitRate: total > 0 ? this.hits / total : 0,
    };
  }

  /**
   * Get all keys
   */
  keys(): string[] {
    return Array.from(this.cache.keys());
  }

  /**
   * Cleanup expired entries
   */
  cleanup(): number {
    const now = Date.now();
    let cleaned = 0;

    for (const [key, entry] of this.cache) {
      if (now > entry.expiresAt) {
        this.delete(key);
        cleaned++;
      }
    }

    return cleaned;
  }

  private evictLRU(): void {
    // Get the first (oldest) entry
    const oldestKey = this.cache.keys().next().value;
    if (oldestKey) {
      this.delete(oldestKey);
    }
  }

  private estimateSize(value: T): number {
    if (value instanceof Uint8Array) {
      return value.byteLength;
    }
    if (typeof value === 'string') {
      return value.length * 2; // UTF-16
    }
    if (typeof value === 'object' && value !== null) {
      return JSON.stringify(value).length * 2;
    }
    return 64; // Default estimate
  }
}

// ============================================================================
// Content Cache
// ============================================================================

/**
 * Specialized cache for ShadowMesh content
 */
export class ContentCache {
  private manifestCache: LRUCache<ContentManifest>;
  private fragmentCache: LRUCache<Uint8Array>;
  private responseCache: LRUCache<Response>;

  constructor(config: CacheConfig = {}) {
    // Split the max size between caches
    const totalSize = config.maxSize ?? 100 * 1024 * 1024;

    this.manifestCache = new LRUCache<ContentManifest>({
      maxSize: Math.floor(totalSize * 0.1), // 10% for manifests
      ttl: config.ttl ?? 3600, // 1 hour default for manifests
    });

    this.fragmentCache = new LRUCache<Uint8Array>({
      maxSize: Math.floor(totalSize * 0.8), // 80% for fragments
      ttl: config.ttl ?? 1800, // 30 minutes default for fragments
    });

    this.responseCache = new LRUCache<Response>({
      maxSize: Math.floor(totalSize * 0.1), // 10% for responses
      ttl: config.ttl ?? 300, // 5 minutes default for responses
    });
  }

  // Manifest operations
  getManifest(cid: string): ContentManifest | null {
    return this.manifestCache.get(`manifest:${cid}`);
  }

  setManifest(cid: string, manifest: ContentManifest, ttl?: number): void {
    this.manifestCache.set(`manifest:${cid}`, manifest, { ttl });
  }

  hasManifest(cid: string): boolean {
    return this.manifestCache.has(`manifest:${cid}`);
  }

  // Fragment operations
  getFragment(cid: string, fragmentId: string): Uint8Array | null {
    return this.fragmentCache.get(`fragment:${cid}:${fragmentId}`);
  }

  setFragment(cid: string, fragmentId: string, data: Uint8Array, ttl?: number): void {
    this.fragmentCache.set(`fragment:${cid}:${fragmentId}`, data, {
      size: data.byteLength,
      ttl,
    });
  }

  hasFragment(cid: string, fragmentId: string): boolean {
    return this.fragmentCache.has(`fragment:${cid}:${fragmentId}`);
  }

  // Response operations (for HTTP caching)
  getResponse(url: string): Response | null {
    return this.responseCache.get(`response:${url}`);
  }

  setResponse(url: string, response: Response, ttl?: number): void {
    this.responseCache.set(`response:${url}`, response, { ttl });
  }

  // General operations
  clear(): void {
    this.manifestCache.clear();
    this.fragmentCache.clear();
    this.responseCache.clear();
  }

  cleanup(): { manifests: number; fragments: number; responses: number } {
    return {
      manifests: this.manifestCache.cleanup(),
      fragments: this.fragmentCache.cleanup(),
      responses: this.responseCache.cleanup(),
    };
  }

  stats(): {
    manifests: CacheStats;
    fragments: CacheStats;
    responses: CacheStats;
    total: CacheStats;
  } {
    const manifests = this.manifestCache.stats();
    const fragments = this.fragmentCache.stats();
    const responses = this.responseCache.stats();

    const totalHits = manifests.hits + fragments.hits + responses.hits;
    const totalMisses = manifests.misses + fragments.misses + responses.misses;
    const total = totalHits + totalMisses;

    return {
      manifests,
      fragments,
      responses,
      total: {
        hits: totalHits,
        misses: totalMisses,
        entries: manifests.entries + fragments.entries + responses.entries,
        bytes: manifests.bytes + fragments.bytes + responses.bytes,
        hitRate: total > 0 ? totalHits / total : 0,
      },
    };
  }
}

// ============================================================================
// Request Deduplication
// ============================================================================

interface PendingRequest<T> {
  promise: Promise<T>;
  timestamp: number;
}

/**
 * Deduplicates concurrent requests to the same resource
 */
export class RequestDeduplicator {
  private pending: Map<string, PendingRequest<unknown>> = new Map();
  private timeout: number;

  constructor(timeout = 30000) {
    this.timeout = timeout;
  }

  /**
   * Deduplicate a request - if same request is pending, return existing promise
   */
  async dedupe<T>(key: string, factory: () => Promise<T>): Promise<T> {
    // Cleanup old pending requests
    this.cleanup();

    // Check for existing request
    const existing = this.pending.get(key) as PendingRequest<T> | undefined;
    if (existing) {
      return existing.promise;
    }

    // Create new request
    const promise = factory().finally(() => {
      this.pending.delete(key);
    });

    this.pending.set(key, {
      promise,
      timestamp: Date.now(),
    });

    return promise;
  }

  /**
   * Cancel a pending request
   */
  cancel(key: string): boolean {
    return this.pending.delete(key);
  }

  /**
   * Get count of pending requests
   */
  get pendingCount(): number {
    return this.pending.size;
  }

  private cleanup(): void {
    const now = Date.now();
    for (const [key, req] of this.pending) {
      if (now - req.timestamp > this.timeout) {
        this.pending.delete(key);
      }
    }
  }
}

// ============================================================================
// Cache Middleware
// ============================================================================

export type FetchFunction = (
  input: RequestInfo | URL,
  init?: RequestInit
) => Promise<Response>;

/**
 * Create a fetch wrapper with caching
 */
export function createCachedFetch(
  cache: ContentCache,
  options: {
    ttl?: number;
    cacheableStatus?: number[];
    cacheableMethods?: string[];
  } = {}
): FetchFunction {
  const {
    ttl = 300,
    cacheableStatus = [200, 203, 204, 206, 300, 301, 308, 404, 405, 410, 414, 501],
    cacheableMethods = ['GET', 'HEAD'],
  } = options;

  return async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
    const method = init?.method ?? 'GET';

    // Check if cacheable
    if (!cacheableMethods.includes(method.toUpperCase())) {
      return fetch(input, init);
    }

    // Check cache
    const cached = cache.getResponse(url);
    if (cached) {
      return cached.clone();
    }

    // Fetch
    const response = await fetch(input, init);

    // Cache if cacheable status
    if (cacheableStatus.includes(response.status)) {
      // Clone response before caching (response body can only be read once)
      cache.setResponse(url, response.clone(), ttl * 1000);
    }

    return response;
  };
}

// ============================================================================
// Preloader
// ============================================================================

/**
 * Preloads content for faster access
 */
export class ContentPreloader {
  private cache: ContentCache;
  private deduplicator: RequestDeduplicator;
  private fetchFn: typeof fetch;

  constructor(
    cache: ContentCache,
    options: { fetchFn?: typeof fetch } = {}
  ) {
    this.cache = cache;
    this.deduplicator = new RequestDeduplicator();
    this.fetchFn = options.fetchFn ?? fetch;
  }

  /**
   * Preload content manifest
   */
  async preloadManifest(gatewayUrl: string, cid: string): Promise<ContentManifest> {
    return this.deduplicator.dedupe(`manifest:${cid}`, async () => {
      // Check cache first
      const cached = this.cache.getManifest(cid);
      if (cached) return cached;

      // Fetch manifest
      const response = await this.fetchFn(`${gatewayUrl}/api/content/${cid}/manifest`);
      if (!response.ok) {
        throw new Error(`Failed to fetch manifest: ${response.status}`);
      }

      const manifest = await response.json() as ContentManifest;
      this.cache.setManifest(cid, manifest);

      return manifest;
    });
  }

  /**
   * Preload content fragments
   */
  async preloadFragments(
    gatewayUrl: string,
    cid: string,
    fragmentIds?: string[],
    concurrency = 4
  ): Promise<Map<string, Uint8Array>> {
    const manifest = await this.preloadManifest(gatewayUrl, cid);
    const fragments = new Map<string, Uint8Array>();

    // Determine which fragments to load
    const toLoad = fragmentIds
      ? manifest.fragments.filter((f) => fragmentIds.includes(f.id))
      : manifest.fragments;

    // Load fragments with concurrency limit
    const chunks: Array<typeof toLoad> = [];
    for (let i = 0; i < toLoad.length; i += concurrency) {
      chunks.push(toLoad.slice(i, i + concurrency));
    }

    for (const chunk of chunks) {
      const results = await Promise.all(
        chunk.map(async (fragment) => {
          // Check cache first
          const cached = this.cache.getFragment(cid, fragment.id);
          if (cached) {
            return { id: fragment.id, data: cached };
          }

          // Fetch fragment
          const response = await this.fetchFn(
            `${gatewayUrl}/api/content/${cid}/fragment/${fragment.id}`
          );
          if (!response.ok) {
            throw new Error(`Failed to fetch fragment ${fragment.id}: ${response.status}`);
          }

          const data = new Uint8Array(await response.arrayBuffer());
          this.cache.setFragment(cid, fragment.id, data);

          return { id: fragment.id, data };
        })
      );

      for (const result of results) {
        fragments.set(result.id, result.data);
      }
    }

    return fragments;
  }

  /**
   * Preload related content (e.g., linked resources)
   */
  async preloadRelated(
    gatewayUrl: string,
    cids: string[],
    manifestsOnly = true
  ): Promise<void> {
    const promises = cids.map((cid) =>
      manifestsOnly
        ? this.preloadManifest(gatewayUrl, cid)
        : this.preloadFragments(gatewayUrl, cid)
    );

    await Promise.allSettled(promises);
  }
}
