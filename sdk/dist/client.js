/**
 * ShadowMesh SDK - API Client Module
 *
 * HTTP client for communicating with ShadowMesh gateway and nodes.
 */
import { ErrorCode, } from './types.js';
import { NameResolver, FALLBACK_GATEWAY_URLS } from './resolver.js';
/**
 * Fallback network endpoints — used ONLY when naming resolution fails.
 * @deprecated Prefer using the decentralized naming layer for gateway discovery.
 */
const NETWORK_ENDPOINTS = FALLBACK_GATEWAY_URLS;
/**
 * Default request timeout (30 seconds)
 */
const DEFAULT_TIMEOUT = 30000;
/**
 * Create a ShadowMesh error
 */
function createError(code, message, statusCode, details) {
    const error = new Error(message);
    error.code = code;
    error.statusCode = statusCode;
    error.details = details;
    return error;
}
/**
 * API Client for ShadowMesh gateway communication
 */
export class GatewayClient {
    baseUrl;
    apiKey;
    timeout;
    debug;
    resolver = null;
    constructor(config = {}) {
        // Explicit URL takes priority
        this.baseUrl = config.gatewayUrl || NETWORK_ENDPOINTS[config.network || 'testnet'];
        this.apiKey = config.apiKey;
        this.timeout = config.timeout || DEFAULT_TIMEOUT;
        this.debug = config.debug || false;
        // Set up resolver for decentralized gateway discovery
        if (!config.gatewayUrl) {
            this.resolver = new NameResolver({
                gatewayUrls: [this.baseUrl], // Use fallback URL as initial gateway for resolution
                bootstrapMultiaddrs: config.bootstrapMultiaddrs,
                enableEns: config.enableEns,
                ensRpcUrl: config.ensRpcUrl,
                timeout: config.timeout,
            });
            // Trigger async gateway resolution (non-blocking)
            this.resolveGateway();
        }
    }
    /**
     * Attempt to resolve a better gateway URL via the naming layer.
     * Updates `baseUrl` if successful; keeps fallback otherwise.
     */
    async resolveGateway() {
        if (!this.resolver)
            return;
        try {
            const url = await this.resolver.resolveGatewayUrl(this.baseUrl);
            if (url && url !== this.baseUrl) {
                if (this.debug) {
                    console.log(`[ShadowMesh] Resolved gateway: ${url}`);
                }
                this.baseUrl = url;
            }
        }
        catch {
            // Keep fallback URL
        }
    }
    /**
     * Make an HTTP request
     */
    async request(method, path, options = {}) {
        const url = `${this.baseUrl}${path}`;
        const controller = new AbortController();
        const timeoutId = setTimeout(() => controller.abort(), options.timeout || this.timeout);
        const headers = {
            'Content-Type': 'application/json',
            ...options.headers,
        };
        if (this.apiKey) {
            headers['Authorization'] = `Bearer ${this.apiKey}`;
        }
        if (this.debug) {
            console.log(`[ShadowMesh] ${method} ${url}`);
        }
        try {
            const response = await fetch(url, {
                method,
                headers,
                body: options.body ? JSON.stringify(options.body) : undefined,
                signal: controller.signal,
            });
            clearTimeout(timeoutId);
            if (!response.ok) {
                const errorBody = await response.text();
                let errorMessage = `Request failed: ${response.status}`;
                let errorCode = ErrorCode.UNKNOWN_ERROR;
                try {
                    const parsed = JSON.parse(errorBody);
                    errorMessage = parsed.error || errorMessage;
                    errorCode = parsed.errorCode || errorCode;
                }
                catch {
                    // Use default error message
                }
                if (response.status === 401) {
                    errorCode = ErrorCode.UNAUTHORIZED;
                }
                else if (response.status === 429) {
                    errorCode = ErrorCode.RATE_LIMITED;
                }
                else if (response.status === 404) {
                    errorCode = ErrorCode.CONTENT_NOT_FOUND;
                }
                throw createError(errorCode, errorMessage, response.status);
            }
            // Handle empty responses
            const text = await response.text();
            if (!text) {
                return {};
            }
            return JSON.parse(text);
        }
        catch (error) {
            clearTimeout(timeoutId);
            if (error instanceof Error) {
                if (error.name === 'AbortError') {
                    throw createError(ErrorCode.TIMEOUT, 'Request timed out');
                }
                if (error.code) {
                    throw error;
                }
            }
            throw createError(ErrorCode.NETWORK_ERROR, `Network error: ${error instanceof Error ? error.message : 'Unknown'}`);
        }
    }
    /**
     * GET request
     */
    async get(path) {
        return this.request('GET', path);
    }
    /**
     * POST request
     */
    async post(path, body) {
        return this.request('POST', path, { body });
    }
    /**
     * PUT request
     */
    async put(path, body) {
        return this.request('PUT', path, { body });
    }
    /**
     * DELETE request
     */
    async delete(path) {
        return this.request('DELETE', path);
    }
    // =========================================================================
    // Health & Status APIs
    // =========================================================================
    /**
     * Check gateway health
     */
    async health() {
        return this.get('/health');
    }
    /**
     * Get gateway metrics
     */
    async metrics() {
        return this.get('/metrics');
    }
    // =========================================================================
    // Content APIs
    // =========================================================================
    /**
     * Get content by CID
     */
    async getContent(cid) {
        const response = await fetch(`${this.baseUrl}/ipfs/${cid}`, {
            headers: this.apiKey ? { Authorization: `Bearer ${this.apiKey}` } : {},
        });
        if (!response.ok) {
            if (response.status === 404) {
                throw createError(ErrorCode.CONTENT_NOT_FOUND, `Content not found: ${cid}`);
            }
            throw createError(ErrorCode.NETWORK_ERROR, `Failed to fetch content: ${response.status}`);
        }
        return response.arrayBuffer();
    }
    /**
     * Get content manifest
     */
    async getManifest(cid) {
        return this.get(`/api/manifest/${cid}`);
    }
    /**
     * Check content status
     */
    async getStatus(cid) {
        return this.get(`/api/status/${cid}`);
    }
    /**
     * Upload content
     */
    async uploadContent(data, manifest) {
        // First upload the raw content
        const uploadResponse = await fetch(`${this.baseUrl}/api/upload`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/octet-stream',
                ...(this.apiKey ? { Authorization: `Bearer ${this.apiKey}` } : {}),
            },
            body: data,
        });
        if (!uploadResponse.ok) {
            throw createError(ErrorCode.UPLOAD_FAILED, `Upload failed: ${uploadResponse.status}`);
        }
        return uploadResponse.json();
    }
    /**
     * Announce content to the network
     */
    async announceContent(manifest, options = {}) {
        await this.post('/api/announce', {
            manifest,
            privacy: options.privacy || 'medium',
            redundancy: options.redundancy || 3,
        });
    }
    /**
     * Pin content
     */
    async pinContent(cid) {
        await this.post(`/api/pin/${cid}`);
    }
    /**
     * Unpin content
     */
    async unpinContent(cid) {
        await this.delete(`/api/pin/${cid}`);
    }
    // =========================================================================
    // Network APIs
    // =========================================================================
    /**
     * Get network statistics
     */
    async networkStats() {
        return this.get('/api/stats');
    }
    /**
     * Get node information
     */
    async nodeInfo() {
        return this.get('/api/node');
    }
    /**
     * Get list of known peers
     */
    async peers() {
        return this.get('/api/peers');
    }
}
/**
 * Node client for direct P2P communication
 */
export class NodeClient {
    baseUrl;
    timeout;
    debug;
    constructor(config = {}) {
        this.baseUrl = config.nodeUrl || 'http://localhost:3030';
        this.timeout = config.timeout || DEFAULT_TIMEOUT;
        this.debug = config.debug || false;
    }
    /**
     * Make a request to the node
     */
    async request(method, path, body) {
        const url = `${this.baseUrl}${path}`;
        if (this.debug) {
            console.log(`[ShadowMesh Node] ${method} ${url}`);
        }
        const response = await fetch(url, {
            method,
            headers: { 'Content-Type': 'application/json' },
            body: body ? JSON.stringify(body) : undefined,
        });
        if (!response.ok) {
            throw createError(ErrorCode.NETWORK_ERROR, `Node request failed: ${response.status}`);
        }
        const text = await response.text();
        return text ? JSON.parse(text) : {};
    }
    /**
     * Get node status
     */
    async status() {
        return this.request('GET', '/api/status');
    }
    /**
     * Get node health
     */
    async health() {
        return this.request('GET', '/api/health');
    }
    /**
     * Get node metrics
     */
    async metrics() {
        return this.request('GET', '/api/metrics');
    }
    /**
     * Get node configuration
     */
    async getConfig() {
        return this.request('GET', '/api/config');
    }
    /**
     * Update node configuration
     */
    async updateConfig(config) {
        return this.request('PUT', '/api/config', {
            max_storage_gb: config.maxStorageGb,
            max_bandwidth_mbps: config.maxBandwidthMbps,
            max_peers: config.maxPeers,
            node_name: config.nodeName,
        });
    }
    /**
     * List stored content
     */
    async listContent() {
        return this.request('GET', '/api/storage/content');
    }
    /**
     * Pin content on node
     */
    async pin(cid) {
        await this.request('POST', `/api/storage/pin/${cid}`);
    }
    /**
     * Unpin content on node
     */
    async unpin(cid) {
        await this.request('POST', `/api/storage/unpin/${cid}`);
    }
    /**
     * Run garbage collection
     */
    async gc(targetFreeGb = 1) {
        return this.request('POST', '/api/storage/gc', { target_free_gb: targetFreeGb });
    }
    /**
     * Shutdown node gracefully
     */
    async shutdown() {
        await this.request('POST', '/api/node/shutdown');
    }
}
/**
 * Create a gateway client
 */
export function createGatewayClient(config) {
    return new GatewayClient(config);
}
/**
 * Create a node client
 */
export function createNodeClient(config) {
    return new NodeClient(config);
}
//# sourceMappingURL=client.js.map