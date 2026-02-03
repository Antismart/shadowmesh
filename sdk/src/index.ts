/**
 * ShadowMesh SDK
 * Decentralized content hosting and delivery network
 */

// Re-export all types
export * from './types.js';

// Re-export client
export {
  GatewayClient,
  NodeClient,
  createGatewayClient,
  createNodeClient,
} from './client.js';

// Re-export crypto utilities
export {
  hashContent,
  hashString,
  deriveKey,
  encrypt,
  decrypt,
  serializeEncrypted,
  deserializeEncrypted,
  verifyHash,
  generateId,
  secureCompare,
  encryptToBase64,
  decryptFromBase64,
  randomBytes,
  type EncryptedData,
} from './crypto.js';

// Re-export storage
export {
  // Backends
  type StorageBackend,
  MemoryStorage,
  IndexedDBStorage,
  // Content storage
  type ContentStorageConfig,
  type StoredContent,
  ContentStorage,
  // Fragment utilities
  reassembleContent,
  fragmentContent,
  // IPFS
  type IPFSConfig,
  IPFSClient,
} from './storage.js';

// Re-export cache
export {
  type CacheEntry,
  type CacheStats,
  LRUCache,
  ContentCache,
  RequestDeduplicator,
  type FetchFunction,
  createCachedFetch,
  ContentPreloader,
} from './cache.js';

// Re-export utilities
export {
  // Size formatting
  formatBytes,
  parseBytes,
  // Time formatting
  formatDuration,
  formatRelativeTime,
  // URL utilities
  buildUrl,
  parseQuery,
  joinPaths,
  // CID utilities
  isValidCid,
  extractCid,
  buildIpfsUrl,
  // Retry & backoff
  type RetryOptions,
  retry,
  // Async utilities
  sleep,
  timeout,
  concurrent,
  debounce,
  throttle,
  // Data utilities
  deepClone,
  deepMerge,
  // Encoding utilities
  bytesToHex,
  hexToBytes,
  bytesToBase64,
  base64ToBytes,
  stringToBytes,
  bytesToString,
  // Validation utilities
  assert,
  ensureDefined,
  isError,
  // Environment utilities
  isBrowser,
  isNode,
  isWebWorker,
  // Event emitter
  EventEmitter,
} from './utils.js';

// Legacy ShadowMesh class (for backwards compatibility)
import { create } from '@storacha/client';
import fs from 'fs/promises';
import path from 'path';

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
export class ShadowMesh {
  private client: ReturnType<typeof create> | null = null;
  private networkEndpoint: string;

  constructor(options: { network?: 'testnet' | 'mainnet' } = {}) {
    this.networkEndpoint = options.network === 'mainnet' 
      ? 'https://api.shadowmesh.network'
      : 'https://testnet.shadowmesh.network';
  }

  async deploy(options: LegacyDeployOptions): Promise<LegacyDeploymentResult> {
    console.log(`📦 Deploying ${options.path}...`);

    // 1. Read and prepare content
    const content = await this.prepareContent(options.path);
    
    // 2. Fragment content
    const manifest = await this.fragmentContent(content, options.path);
    
    // 3. Upload to IPFS/Filecoin
    const cid = await this.uploadToStorage(content);
    
    // 4. Announce to ShadowMesh network
    await this.announceToNetwork(manifest, options);
    
    // 5. Generate URLs
    const result: LegacyDeploymentResult = {
      gateway: `https://${cid}.shadowmesh.network`,
      native: `shadow://${cid}`,
      cid,
      manifest,
    };

    if (options.ens) {
      result.ens = await this.registerENS(options.ens, cid);
    }

    console.log(`✅ Deployed successfully!`);
    return result;
  }

  private async prepareContent(filePath: string): Promise<Buffer> {
    const stats = await fs.stat(filePath);
    
    if (stats.isDirectory()) {
      // TODO: Handle directory deployment (zip or individual files)
      throw new Error('Directory deployment not yet implemented');
    }
    
    return fs.readFile(filePath);
  }

  private async fragmentContent(content: Buffer, filePath: string): Promise<LegacyContentManifest> {
    try {
      // Call Rust protocol via HTTP API
      const response = await fetch(`${this.networkEndpoint}/fragment`, {
        method: 'POST',
        body: new Uint8Array(content),
      });
      
      if (response.ok) {
        return response.json();
      }
    } catch {
      // Fallback to local fragmentation if network unavailable
    }

    // Local fallback fragmentation
    const CHUNK_SIZE = 256 * 1024; // 256KB
    const fragments: string[] = [];
    
    for (let i = 0; i < content.length; i += CHUNK_SIZE) {
      const chunk = content.slice(i, i + CHUNK_SIZE);
      // Simple hash placeholder - in production use blake3
      const hash = Buffer.from(chunk).toString('base64').slice(0, 32);
      fragments.push(hash);
    }

    return {
      content_hash: Buffer.from(content).toString('base64').slice(0, 64),
      fragments,
      metadata: {
        name: path.basename(filePath),
        size: content.length,
        mime_type: 'application/octet-stream',
      },
    };
  }

  private async uploadToStorage(content: Buffer): Promise<string> {
    try {
      // Initialize w3up client
      const client = await create();
      
      // Upload to IPFS via web3.storage
      const file = new File([new Uint8Array(content)], 'content');
      const cid = await client.uploadFile(file);
      
      return cid.toString();
    } catch (error) {
      // Fallback CID generation for development
      console.warn('⚠️  Web3.storage upload failed, using local CID');
      const hash = Buffer.from(content).toString('base64').slice(0, 46);
      return `baf${hash}`;
    }
  }

  private async announceToNetwork(
    manifest: LegacyContentManifest, 
    options: LegacyDeployOptions
  ): Promise<void> {
    try {
      // Announce content to ShadowMesh network
      await fetch(`${this.networkEndpoint}/announce`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          manifest,
          privacy: options.privacy || 'medium',
          redundancy: options.redundancy || 5,
        }),
      });
    } catch {
      console.warn('⚠️  Network announcement failed, content available via IPFS only');
    }
  }

  private async registerENS(ensName: string, cid: string): Promise<string> {
    // TODO: Implement ENS registration
    console.log(`📝 ENS registration for ${ensName} (coming soon)`);
    return `https://${ensName}.limo`;
  }

  /**
   * Check the status of a deployment
   */
  async status(cid: string): Promise<{ available: boolean; replicas: number }> {
    try {
      const response = await fetch(`${this.networkEndpoint}/status/${cid}`);
      if (response.ok) {
        return response.json();
      }
    } catch {
      // Network unavailable
    }
    return { available: false, replicas: 0 };
  }

  /**
   * Get network statistics
   */
  async networkStats(): Promise<{ nodes: number; bandwidth: number; files: number }> {
    try {
      const response = await fetch(`${this.networkEndpoint}/stats`);
      if (response.ok) {
        return response.json();
      }
    } catch {
      // Network unavailable
    }
    return { nodes: 0, bandwidth: 0, files: 0 };
  }
}

// Default export for convenience
export default ShadowMesh;

