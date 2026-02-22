/**
 * ShadowMesh SDK - Storage Module
 * Client-side storage and IPFS integration
 */

import type {
  ContentManifest,
  FragmentInfo,
  StorageStats,
} from './types.js';

// ============================================================================
// Storage Backends
// ============================================================================

export interface StorageBackend {
  get(key: string): Promise<Uint8Array | null>;
  set(key: string, value: Uint8Array): Promise<void>;
  delete(key: string): Promise<boolean>;
  has(key: string): Promise<boolean>;
  list(prefix?: string): Promise<string[]>;
  clear(): Promise<void>;
  stats(): Promise<{ keys: number; bytes: number }>;
}

/**
 * In-memory storage backend for testing and ephemeral use
 */
export class MemoryStorage implements StorageBackend {
  private store: Map<string, Uint8Array> = new Map();

  async get(key: string): Promise<Uint8Array | null> {
    return this.store.get(key) ?? null;
  }

  async set(key: string, value: Uint8Array): Promise<void> {
    this.store.set(key, value);
  }

  async delete(key: string): Promise<boolean> {
    return this.store.delete(key);
  }

  async has(key: string): Promise<boolean> {
    return this.store.has(key);
  }

  async list(prefix?: string): Promise<string[]> {
    const keys = Array.from(this.store.keys());
    if (!prefix) return keys;
    return keys.filter((k) => k.startsWith(prefix));
  }

  async clear(): Promise<void> {
    this.store.clear();
  }

  async stats(): Promise<{ keys: number; bytes: number }> {
    let bytes = 0;
    for (const value of this.store.values()) {
      bytes += value.byteLength;
    }
    return { keys: this.store.size, bytes };
  }
}

/**
 * IndexedDB storage backend for browser persistence
 */
export class IndexedDBStorage implements StorageBackend {
  private dbName: string;
  private storeName: string;
  private db: IDBDatabase | null = null;

  constructor(dbName = 'shadowmesh', storeName = 'content') {
    this.dbName = dbName;
    this.storeName = storeName;
  }

  private async getDB(): Promise<IDBDatabase> {
    if (this.db) return this.db;

    return new Promise((resolve, reject) => {
      const request = indexedDB.open(this.dbName, 1);

      request.onerror = () => reject(request.error);
      request.onsuccess = () => {
        this.db = request.result;
        resolve(this.db);
      };

      request.onupgradeneeded = (event) => {
        const db = (event.target as IDBOpenDBRequest).result;
        if (!db.objectStoreNames.contains(this.storeName)) {
          db.createObjectStore(this.storeName);
        }
      };
    });
  }

  async get(key: string): Promise<Uint8Array | null> {
    const db = await this.getDB();
    return new Promise((resolve, reject) => {
      const tx = db.transaction(this.storeName, 'readonly');
      const store = tx.objectStore(this.storeName);
      const request = store.get(key);

      request.onerror = () => reject(request.error);
      request.onsuccess = () => resolve(request.result ?? null);
    });
  }

  async set(key: string, value: Uint8Array): Promise<void> {
    const db = await this.getDB();
    return new Promise((resolve, reject) => {
      const tx = db.transaction(this.storeName, 'readwrite');
      const store = tx.objectStore(this.storeName);
      const request = store.put(value, key);

      request.onerror = () => reject(request.error);
      request.onsuccess = () => resolve();
    });
  }

  async delete(key: string): Promise<boolean> {
    const db = await this.getDB();
    return new Promise((resolve, reject) => {
      const tx = db.transaction(this.storeName, 'readwrite');
      const store = tx.objectStore(this.storeName);
      const request = store.delete(key);

      request.onerror = () => reject(request.error);
      request.onsuccess = () => resolve(true);
    });
  }

  async has(key: string): Promise<boolean> {
    const value = await this.get(key);
    return value !== null;
  }

  async list(_prefix?: string): Promise<string[]> {
    const db = await this.getDB();
    return new Promise((resolve, reject) => {
      const tx = db.transaction(this.storeName, 'readonly');
      const store = tx.objectStore(this.storeName);
      const request = store.getAllKeys();

      request.onerror = () => reject(request.error);
      request.onsuccess = () => {
        const keys = request.result as string[];
        if (!_prefix) {
          resolve(keys);
        } else {
          resolve(keys.filter((k) => k.startsWith(_prefix)));
        }
      };
    });
  }

  async clear(): Promise<void> {
    const db = await this.getDB();
    return new Promise((resolve, reject) => {
      const tx = db.transaction(this.storeName, 'readwrite');
      const store = tx.objectStore(this.storeName);
      const request = store.clear();

      request.onerror = () => reject(request.error);
      request.onsuccess = () => resolve();
    });
  }

  async stats(): Promise<{ keys: number; bytes: number }> {
    const keys = await this.list();
    let bytes = 0;
    for (const key of keys) {
      const value = await this.get(key);
      if (value) bytes += value.byteLength;
    }
    return { keys: keys.length, bytes };
  }

  close(): void {
    if (this.db) {
      this.db.close();
      this.db = null;
    }
  }
}

// ============================================================================
// Content Storage Manager
// ============================================================================

export interface ContentStorageConfig {
  backend: StorageBackend;
  maxSize?: number; // Maximum storage size in bytes
  gcThreshold?: number; // Threshold for triggering GC (0-1)
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
export class ContentStorage {
  private backend: StorageBackend;
  private maxSize: number;
  private gcThreshold: number;
  private manifests: Map<string, ContentManifest> = new Map();

  constructor(config: ContentStorageConfig) {
    this.backend = config.backend;
    this.maxSize = config.maxSize ?? 100 * 1024 * 1024; // 100MB default
    this.gcThreshold = config.gcThreshold ?? 0.9;
  }

  /**
   * Store content with its manifest
   */
  async storeContent(
    cid: string,
    manifest: ContentManifest,
    fragments: Map<string, Uint8Array>
  ): Promise<void> {
    // Store manifest
    const manifestKey = `manifest:${cid}`;
    const manifestData = new TextEncoder().encode(JSON.stringify(manifest));
    await this.backend.set(manifestKey, manifestData);
    this.manifests.set(cid, manifest);

    // Store fragments
    for (const [fragmentId, data] of fragments) {
      const fragmentKey = `fragment:${cid}:${fragmentId}`;
      await this.backend.set(fragmentKey, data);
    }

    // Store metadata
    const metaKey = `meta:${cid}`;
    const meta = {
      storedAt: new Date().toISOString(),
      lastAccessed: new Date().toISOString(),
      pinned: false,
    };
    await this.backend.set(metaKey, new TextEncoder().encode(JSON.stringify(meta)));

    // Check if GC is needed
    await this.maybeRunGC();
  }

  /**
   * Retrieve content by CID
   */
  async getContent(cid: string): Promise<{
    manifest: ContentManifest;
    fragments: Map<string, Uint8Array>;
  } | null> {
    // Get manifest
    const manifestKey = `manifest:${cid}`;
    const manifestData = await this.backend.get(manifestKey);
    if (!manifestData) return null;

    const manifest = JSON.parse(
      new TextDecoder().decode(manifestData)
    ) as ContentManifest;

    // Get fragments
    const fragments = new Map<string, Uint8Array>();
    for (const fragment of manifest.fragments) {
      const fragmentKey = `fragment:${cid}:${fragment.id}`;
      const data = await this.backend.get(fragmentKey);
      if (data) {
        fragments.set(fragment.id, data);
      }
    }

    // Update last accessed
    await this.updateLastAccessed(cid);

    return { manifest, fragments };
  }

  /**
   * Get manifest only (without fragments)
   */
  async getManifest(cid: string): Promise<ContentManifest | null> {
    // Check cache first
    if (this.manifests.has(cid)) {
      return this.manifests.get(cid)!;
    }

    const manifestKey = `manifest:${cid}`;
    const manifestData = await this.backend.get(manifestKey);
    if (!manifestData) return null;

    const manifest = JSON.parse(
      new TextDecoder().decode(manifestData)
    ) as ContentManifest;
    this.manifests.set(cid, manifest);

    return manifest;
  }

  /**
   * Get a specific fragment
   */
  async getFragment(cid: string, fragmentId: string): Promise<Uint8Array | null> {
    const fragmentKey = `fragment:${cid}:${fragmentId}`;
    return this.backend.get(fragmentKey);
  }

  /**
   * Store a single fragment
   */
  async storeFragment(
    cid: string,
    fragmentId: string,
    data: Uint8Array
  ): Promise<void> {
    const fragmentKey = `fragment:${cid}:${fragmentId}`;
    await this.backend.set(fragmentKey, data);
  }

  /**
   * Check if content exists
   */
  async hasContent(cid: string): Promise<boolean> {
    const manifestKey = `manifest:${cid}`;
    return this.backend.has(manifestKey);
  }

  /**
   * Check if a specific fragment exists
   */
  async hasFragment(cid: string, fragmentId: string): Promise<boolean> {
    const fragmentKey = `fragment:${cid}:${fragmentId}`;
    return this.backend.has(fragmentKey);
  }

  /**
   * Delete content and all its fragments
   */
  async deleteContent(cid: string): Promise<boolean> {
    // Check if pinned
    const meta = await this.getMetadata(cid);
    if (meta?.pinned) {
      return false;
    }

    // Get manifest to know which fragments to delete
    const manifest = await this.getManifest(cid);
    if (!manifest) return false;

    // Delete fragments
    for (const fragment of manifest.fragments) {
      const fragmentKey = `fragment:${cid}:${fragment.id}`;
      await this.backend.delete(fragmentKey);
    }

    // Delete manifest and metadata
    await this.backend.delete(`manifest:${cid}`);
    await this.backend.delete(`meta:${cid}`);
    this.manifests.delete(cid);

    return true;
  }

  /**
   * Pin content to prevent GC
   */
  async pinContent(cid: string): Promise<void> {
    const meta = await this.getMetadata(cid);
    if (meta) {
      meta.pinned = true;
      await this.setMetadata(cid, meta);
    }
  }

  /**
   * Unpin content to allow GC
   */
  async unpinContent(cid: string): Promise<void> {
    const meta = await this.getMetadata(cid);
    if (meta) {
      meta.pinned = false;
      await this.setMetadata(cid, meta);
    }
  }

  /**
   * List all stored content CIDs
   */
  async listContent(): Promise<string[]> {
    const keys = await this.backend.list('manifest:');
    return keys.map((k) => k.replace('manifest:', ''));
  }

  /**
   * Get storage statistics
   */
  async getStats(): Promise<StorageStats> {
    const backendStats = await this.backend.stats();
    const cids = await this.listContent();

    let totalFragments = 0;
    for (const cid of cids) {
      const manifest = await this.getManifest(cid);
      if (manifest) {
        totalFragments += manifest.fragments.length;
      }
    }

    return {
      totalBytes: backendStats.bytes,
      usedBytes: backendStats.bytes,
      availableBytes: this.maxSize - backendStats.bytes,
      contentCount: cids.length,
      fragmentCount: totalFragments,
    };
  }

  /**
   * Run garbage collection
   */
  async runGC(targetBytes?: number): Promise<{ freed: number; deleted: string[] }> {
    const stats = await this.backend.stats();
    const target = targetBytes ?? Math.floor(this.maxSize * 0.7);
    const toFree = stats.bytes - target;

    if (toFree <= 0) {
      return { freed: 0, deleted: [] };
    }

    // Get all content with metadata, sorted by last accessed
    const cids = await this.listContent();
    const contentMeta: Array<{
      cid: string;
      lastAccessed: Date;
      size: number;
      pinned: boolean;
    }> = [];

    for (const cid of cids) {
      const meta = await this.getMetadata(cid);
      const manifest = await this.getManifest(cid);

      if (meta && manifest && !meta.pinned) {
        contentMeta.push({
          cid,
          lastAccessed: new Date(meta.lastAccessed),
          size: manifest.size,
          pinned: meta.pinned,
        });
      }
    }

    // Sort by last accessed (oldest first)
    contentMeta.sort((a, b) => a.lastAccessed.getTime() - b.lastAccessed.getTime());

    // Delete until we've freed enough
    let freed = 0;
    const deleted: string[] = [];

    for (const content of contentMeta) {
      if (freed >= toFree) break;

      await this.deleteContent(content.cid);
      freed += content.size;
      deleted.push(content.cid);
    }

    return { freed, deleted };
  }

  private async maybeRunGC(): Promise<void> {
    const stats = await this.backend.stats();
    if (stats.bytes > this.maxSize * this.gcThreshold) {
      await this.runGC();
    }
  }

  private async getMetadata(cid: string): Promise<{
    storedAt: string;
    lastAccessed: string;
    pinned: boolean;
  } | null> {
    const metaKey = `meta:${cid}`;
    const data = await this.backend.get(metaKey);
    if (!data) return null;
    return JSON.parse(new TextDecoder().decode(data));
  }

  private async setMetadata(
    cid: string,
    meta: { storedAt: string; lastAccessed: string; pinned: boolean }
  ): Promise<void> {
    const metaKey = `meta:${cid}`;
    await this.backend.set(metaKey, new TextEncoder().encode(JSON.stringify(meta)));
  }

  private async updateLastAccessed(cid: string): Promise<void> {
    const meta = await this.getMetadata(cid);
    if (meta) {
      meta.lastAccessed = new Date().toISOString();
      await this.setMetadata(cid, meta);
    }
  }
}

// ============================================================================
// Fragment Reassembly
// ============================================================================

/**
 * Reassemble content from fragments
 */
export function reassembleContent(
  manifest: ContentManifest,
  fragments: Map<string, Uint8Array>
): Uint8Array {
  // Sort fragments by index
  const sortedFragments = [...manifest.fragments].sort((a, b) => a.index - b.index);

  // Calculate total size
  let totalSize = 0;
  for (const fragment of sortedFragments) {
    const data = fragments.get(fragment.id);
    if (!data) {
      throw new Error(`Missing fragment: ${fragment.id}`);
    }
    totalSize += data.byteLength;
  }

  // Reassemble
  const result = new Uint8Array(totalSize);
  let offset = 0;

  for (const fragment of sortedFragments) {
    const data = fragments.get(fragment.id)!;
    result.set(data, offset);
    offset += data.byteLength;
  }

  return result;
}

/**
 * Split content into fragments
 */
export function fragmentContent(
  data: Uint8Array,
  fragmentSize: number = 256 * 1024 // 256KB default
): Array<{ index: number; data: Uint8Array }> {
  const fragments: Array<{ index: number; data: Uint8Array }> = [];
  let offset = 0;
  let index = 0;

  while (offset < data.byteLength) {
    const end = Math.min(offset + fragmentSize, data.byteLength);
    const fragment = data.slice(offset, end);
    fragments.push({ index, data: fragment });
    offset = end;
    index++;
  }

  return fragments;
}

// ============================================================================
// IPFS Integration
// ============================================================================

export interface IPFSConfig {
  gateway: string;
  apiUrl?: string;
  timeout?: number;
}

/**
 * IPFS client for content retrieval
 */
export class IPFSClient {
  private gateway: string;
  private apiUrl: string | null;
  private timeout: number;

  constructor(config: IPFSConfig) {
    this.gateway = config.gateway.replace(/\/$/, '');
    this.apiUrl = config.apiUrl?.replace(/\/$/, '') ?? null;
    this.timeout = config.timeout ?? 30000;
  }

  /**
   * Fetch content from IPFS gateway
   */
  async get(cid: string): Promise<Uint8Array> {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), this.timeout);

    try {
      const response = await fetch(`${this.gateway}/ipfs/${cid}`, {
        signal: controller.signal,
      });

      if (!response.ok) {
        throw new Error(`IPFS fetch failed: ${response.status}`);
      }

      const buffer = await response.arrayBuffer();
      return new Uint8Array(buffer);
    } finally {
      clearTimeout(timeoutId);
    }
  }

  /**
   * Check if content exists on IPFS
   */
  async exists(cid: string): Promise<boolean> {
    try {
      const response = await fetch(`${this.gateway}/ipfs/${cid}`, {
        method: 'HEAD',
      });
      return response.ok;
    } catch {
      return false;
    }
  }

  /**
   * Add content to IPFS (requires API access)
   */
  async add(data: Uint8Array): Promise<string> {
    if (!this.apiUrl) {
      throw new Error('IPFS API URL not configured');
    }

    const formData = new FormData();
    formData.append('file', new Blob([data.buffer as ArrayBuffer]));

    const response = await fetch(`${this.apiUrl}/api/v0/add`, {
      method: 'POST',
      body: formData,
    });

    if (!response.ok) {
      throw new Error(`IPFS add failed: ${response.status}`);
    }

    const result = await response.json();
    return result.Hash;
  }

  /**
   * Pin content on IPFS (requires API access)
   */
  async pin(cid: string): Promise<void> {
    if (!this.apiUrl) {
      throw new Error('IPFS API URL not configured');
    }

    const response = await fetch(`${this.apiUrl}/api/v0/pin/add?arg=${encodeURIComponent(cid)}`, {
      method: 'POST',
    });

    if (!response.ok) {
      throw new Error(`IPFS pin failed: ${response.status}`);
    }
  }

  /**
   * Unpin content from IPFS (requires API access)
   */
  async unpin(cid: string): Promise<void> {
    if (!this.apiUrl) {
      throw new Error('IPFS API URL not configured');
    }

    const response = await fetch(`${this.apiUrl}/api/v0/pin/rm?arg=${encodeURIComponent(cid)}`, {
      method: 'POST',
    });

    if (!response.ok) {
      throw new Error(`IPFS unpin failed: ${response.status}`);
    }
  }
}
