/**
 * ShadowMesh SDK
 * Decentralized content hosting and delivery network
 */
// Re-export all types
export * from './types.js';
// Re-export client
export { GatewayClient, NodeClient, createGatewayClient, createNodeClient, } from './client.js';
// Re-export resolver (decentralized naming)
export { NameResolver, BOOTSTRAP_MULTIADDRS, FALLBACK_GATEWAY_URLS, WELL_KNOWN_NAMES, } from './resolver.js';
// Re-export crypto utilities
export { hashContent, hashString, deriveKey, encrypt, decrypt, serializeEncrypted, deserializeEncrypted, verifyHash, generateId, secureCompare, encryptToBase64, decryptFromBase64, randomBytes, } from './crypto.js';
// Re-export storage
export { MemoryStorage, IndexedDBStorage, ContentStorage, 
// Fragment utilities
reassembleContent, fragmentContent, IPFSClient, } from './storage.js';
// Re-export cache
export { LRUCache, ContentCache, RequestDeduplicator, createCachedFetch, ContentPreloader, } from './cache.js';
// Re-export utilities
export { 
// Size formatting
formatBytes, parseBytes, 
// Time formatting
formatDuration, formatRelativeTime, 
// URL utilities
buildUrl, parseQuery, joinPaths, 
// CID utilities
isValidCid, extractCid, buildIpfsUrl, retry, 
// Async utilities
sleep, timeout, concurrent, debounce, throttle, 
// Data utilities
deepClone, deepMerge, 
// Encoding utilities
bytesToHex, hexToBytes, bytesToBase64, base64ToBytes, stringToBytes, bytesToString, 
// Validation utilities
assert, ensureDefined, isError, 
// Environment utilities
isBrowser, isNode, isWebWorker, 
// Event emitter
EventEmitter, } from './utils.js';
// Legacy ShadowMesh class (for backwards compatibility)
import { create } from '@storacha/client';
import fs from 'fs/promises';
import path from 'path';
/**
 * @deprecated Use ShadowMeshClient instead
 */
export class ShadowMesh {
    client = null;
    networkEndpoint;
    constructor(options = {}) {
        this.networkEndpoint = options.network === 'mainnet'
            ? 'https://api.shadowmesh.network'
            : 'https://testnet.shadowmesh.network';
    }
    async deploy(options) {
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
        const result = {
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
    async prepareContent(filePath) {
        const stats = await fs.stat(filePath);
        if (stats.isDirectory()) {
            // TODO: Handle directory deployment (zip or individual files)
            throw new Error('Directory deployment not yet implemented');
        }
        return fs.readFile(filePath);
    }
    async fragmentContent(content, filePath) {
        try {
            // Call Rust protocol via HTTP API
            const response = await fetch(`${this.networkEndpoint}/fragment`, {
                method: 'POST',
                body: new Uint8Array(content),
            });
            if (response.ok) {
                return response.json();
            }
        }
        catch {
            // Fallback to local fragmentation if network unavailable
        }
        // Local fallback fragmentation
        const CHUNK_SIZE = 256 * 1024; // 256KB
        const fragments = [];
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
    async uploadToStorage(content) {
        try {
            // Initialize w3up client
            const client = await create();
            // Upload to IPFS via web3.storage
            const file = new File([new Uint8Array(content)], 'content');
            const cid = await client.uploadFile(file);
            return cid.toString();
        }
        catch (error) {
            // Fallback CID generation for development
            console.warn('⚠️  Web3.storage upload failed, using local CID');
            const hash = Buffer.from(content).toString('base64').slice(0, 46);
            return `baf${hash}`;
        }
    }
    async announceToNetwork(manifest, options) {
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
        }
        catch {
            console.warn('⚠️  Network announcement failed, content available via IPFS only');
        }
    }
    async registerENS(ensName, cid) {
        // TODO: Implement ENS registration
        console.log(`📝 ENS registration for ${ensName} (coming soon)`);
        return `https://${ensName}.limo`;
    }
    /**
     * Check the status of a deployment
     */
    async status(cid) {
        try {
            const response = await fetch(`${this.networkEndpoint}/status/${cid}`);
            if (response.ok) {
                return response.json();
            }
        }
        catch {
            // Network unavailable
        }
        return { available: false, replicas: 0 };
    }
    /**
     * Get network statistics
     */
    async networkStats() {
        try {
            const response = await fetch(`${this.networkEndpoint}/stats`);
            if (response.ok) {
                return response.json();
            }
        }
        catch {
            // Network unavailable
        }
        return { nodes: 0, bandwidth: 0, files: 0 };
    }
}
// Default export for convenience
export default ShadowMesh;
//# sourceMappingURL=index.js.map