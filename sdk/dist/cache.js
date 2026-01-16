/**
 * ShadowMesh SDK - Cache Module
 * Client-side caching for content and metadata
 */
// ============================================================================
// LRU Cache Implementation
// ============================================================================
/**
 * Generic LRU (Least Recently Used) cache
 */
export class LRUCache {
    cache = new Map();
    maxSize;
    defaultTTL;
    currentSize = 0;
    hits = 0;
    misses = 0;
    constructor(config = {}) {
        this.maxSize = config.maxSize ?? 50 * 1024 * 1024; // 50MB default
        this.defaultTTL = (config.ttl ?? 300) * 1000; // Convert to ms
    }
    /**
     * Get a value from the cache
     */
    get(key) {
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
    set(key, value, options) {
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
        const entry = {
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
    has(key) {
        const entry = this.cache.get(key);
        if (!entry)
            return false;
        if (Date.now() > entry.expiresAt) {
            this.delete(key);
            return false;
        }
        return true;
    }
    /**
     * Delete a key from the cache
     */
    delete(key) {
        const entry = this.cache.get(key);
        if (!entry)
            return false;
        this.currentSize -= entry.size;
        return this.cache.delete(key);
    }
    /**
     * Clear all entries
     */
    clear() {
        this.cache.clear();
        this.currentSize = 0;
    }
    /**
     * Get cache statistics
     */
    stats() {
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
    keys() {
        return Array.from(this.cache.keys());
    }
    /**
     * Cleanup expired entries
     */
    cleanup() {
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
    evictLRU() {
        // Get the first (oldest) entry
        const oldestKey = this.cache.keys().next().value;
        if (oldestKey) {
            this.delete(oldestKey);
        }
    }
    estimateSize(value) {
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
    manifestCache;
    fragmentCache;
    responseCache;
    constructor(config = {}) {
        // Split the max size between caches
        const totalSize = config.maxSize ?? 100 * 1024 * 1024;
        this.manifestCache = new LRUCache({
            maxSize: Math.floor(totalSize * 0.1), // 10% for manifests
            ttl: config.ttl ?? 3600, // 1 hour default for manifests
        });
        this.fragmentCache = new LRUCache({
            maxSize: Math.floor(totalSize * 0.8), // 80% for fragments
            ttl: config.ttl ?? 1800, // 30 minutes default for fragments
        });
        this.responseCache = new LRUCache({
            maxSize: Math.floor(totalSize * 0.1), // 10% for responses
            ttl: config.ttl ?? 300, // 5 minutes default for responses
        });
    }
    // Manifest operations
    getManifest(cid) {
        return this.manifestCache.get(`manifest:${cid}`);
    }
    setManifest(cid, manifest, ttl) {
        this.manifestCache.set(`manifest:${cid}`, manifest, { ttl });
    }
    hasManifest(cid) {
        return this.manifestCache.has(`manifest:${cid}`);
    }
    // Fragment operations
    getFragment(cid, fragmentId) {
        return this.fragmentCache.get(`fragment:${cid}:${fragmentId}`);
    }
    setFragment(cid, fragmentId, data, ttl) {
        this.fragmentCache.set(`fragment:${cid}:${fragmentId}`, data, {
            size: data.byteLength,
            ttl,
        });
    }
    hasFragment(cid, fragmentId) {
        return this.fragmentCache.has(`fragment:${cid}:${fragmentId}`);
    }
    // Response operations (for HTTP caching)
    getResponse(url) {
        return this.responseCache.get(`response:${url}`);
    }
    setResponse(url, response, ttl) {
        this.responseCache.set(`response:${url}`, response, { ttl });
    }
    // General operations
    clear() {
        this.manifestCache.clear();
        this.fragmentCache.clear();
        this.responseCache.clear();
    }
    cleanup() {
        return {
            manifests: this.manifestCache.cleanup(),
            fragments: this.fragmentCache.cleanup(),
            responses: this.responseCache.cleanup(),
        };
    }
    stats() {
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
/**
 * Deduplicates concurrent requests to the same resource
 */
export class RequestDeduplicator {
    pending = new Map();
    timeout;
    constructor(timeout = 30000) {
        this.timeout = timeout;
    }
    /**
     * Deduplicate a request - if same request is pending, return existing promise
     */
    async dedupe(key, factory) {
        // Cleanup old pending requests
        this.cleanup();
        // Check for existing request
        const existing = this.pending.get(key);
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
    cancel(key) {
        return this.pending.delete(key);
    }
    /**
     * Get count of pending requests
     */
    get pendingCount() {
        return this.pending.size;
    }
    cleanup() {
        const now = Date.now();
        for (const [key, req] of this.pending) {
            if (now - req.timestamp > this.timeout) {
                this.pending.delete(key);
            }
        }
    }
}
/**
 * Create a fetch wrapper with caching
 */
export function createCachedFetch(cache, options = {}) {
    const { ttl = 300, cacheableStatus = [200, 203, 204, 206, 300, 301, 308, 404, 405, 410, 414, 501], cacheableMethods = ['GET', 'HEAD'], } = options;
    return async (input, init) => {
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
    cache;
    deduplicator;
    fetchFn;
    constructor(cache, options = {}) {
        this.cache = cache;
        this.deduplicator = new RequestDeduplicator();
        this.fetchFn = options.fetchFn ?? fetch;
    }
    /**
     * Preload content manifest
     */
    async preloadManifest(gatewayUrl, cid) {
        return this.deduplicator.dedupe(`manifest:${cid}`, async () => {
            // Check cache first
            const cached = this.cache.getManifest(cid);
            if (cached)
                return cached;
            // Fetch manifest
            const response = await this.fetchFn(`${gatewayUrl}/api/content/${cid}/manifest`);
            if (!response.ok) {
                throw new Error(`Failed to fetch manifest: ${response.status}`);
            }
            const manifest = await response.json();
            this.cache.setManifest(cid, manifest);
            return manifest;
        });
    }
    /**
     * Preload content fragments
     */
    async preloadFragments(gatewayUrl, cid, fragmentIds, concurrency = 4) {
        const manifest = await this.preloadManifest(gatewayUrl, cid);
        const fragments = new Map();
        // Determine which fragments to load
        const toLoad = fragmentIds
            ? manifest.fragments.filter((f) => fragmentIds.includes(f.id))
            : manifest.fragments;
        // Load fragments with concurrency limit
        const chunks = [];
        for (let i = 0; i < toLoad.length; i += concurrency) {
            chunks.push(toLoad.slice(i, i + concurrency));
        }
        for (const chunk of chunks) {
            const results = await Promise.all(chunk.map(async (fragment) => {
                // Check cache first
                const cached = this.cache.getFragment(cid, fragment.id);
                if (cached) {
                    return { id: fragment.id, data: cached };
                }
                // Fetch fragment
                const response = await this.fetchFn(`${gatewayUrl}/api/content/${cid}/fragment/${fragment.id}`);
                if (!response.ok) {
                    throw new Error(`Failed to fetch fragment ${fragment.id}: ${response.status}`);
                }
                const data = new Uint8Array(await response.arrayBuffer());
                this.cache.setFragment(cid, fragment.id, data);
                return { id: fragment.id, data };
            }));
            for (const result of results) {
                fragments.set(result.id, result.data);
            }
        }
        return fragments;
    }
    /**
     * Preload related content (e.g., linked resources)
     */
    async preloadRelated(gatewayUrl, cids, manifestsOnly = true) {
        const promises = cids.map((cid) => manifestsOnly
            ? this.preloadManifest(gatewayUrl, cid)
            : this.preloadFragments(gatewayUrl, cid));
        await Promise.allSettled(promises);
    }
}
//# sourceMappingURL=cache.js.map