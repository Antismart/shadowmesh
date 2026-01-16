/**
 * ShadowMesh SDK - API Client Module
 * 
 * HTTP client for communicating with ShadowMesh gateway and nodes.
 */

import {
  ErrorCode,
} from './types.js';

import type {
  ShadowMeshConfig,
  ContentManifest,
  DeploymentStatus,
  NetworkStats,
  NodeInfo,
  GatewayHealth,
  GatewayMetrics,
  ShadowMeshError,
} from './types.js';

/**
 * Default network endpoints
 */
const NETWORK_ENDPOINTS = {
  mainnet: 'https://api.shadowmesh.network',
  testnet: 'https://testnet.shadowmesh.network',
  local: 'http://localhost:3000',
} as const;

/**
 * Default request timeout (30 seconds)
 */
const DEFAULT_TIMEOUT = 30000;

/**
 * Create a ShadowMesh error
 */
function createError(
  code: ErrorCode,
  message: string,
  statusCode?: number,
  details?: Record<string, unknown>
): ShadowMeshError {
  const error = new Error(message) as ShadowMeshError;
  error.code = code;
  error.statusCode = statusCode;
  error.details = details;
  return error;
}

/**
 * API Client for ShadowMesh gateway communication
 */
export class GatewayClient {
  private baseUrl: string;
  private apiKey?: string;
  private timeout: number;
  private debug: boolean;

  constructor(config: ShadowMeshConfig = {}) {
    this.baseUrl = config.gatewayUrl || NETWORK_ENDPOINTS[config.network || 'testnet'];
    this.apiKey = config.apiKey;
    this.timeout = config.timeout || DEFAULT_TIMEOUT;
    this.debug = config.debug || false;
  }

  /**
   * Make an HTTP request
   */
  private async request<T>(
    method: string,
    path: string,
    options: {
      body?: unknown;
      headers?: Record<string, string>;
      timeout?: number;
    } = {}
  ): Promise<T> {
    const url = `${this.baseUrl}${path}`;
    const controller = new AbortController();
    const timeoutId = setTimeout(
      () => controller.abort(),
      options.timeout || this.timeout
    );

    const headers: Record<string, string> = {
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
        } catch {
          // Use default error message
        }

        if (response.status === 401) {
          errorCode = ErrorCode.UNAUTHORIZED;
        } else if (response.status === 429) {
          errorCode = ErrorCode.RATE_LIMITED;
        } else if (response.status === 404) {
          errorCode = ErrorCode.CONTENT_NOT_FOUND;
        }

        throw createError(errorCode, errorMessage, response.status);
      }

      // Handle empty responses
      const text = await response.text();
      if (!text) {
        return {} as T;
      }

      return JSON.parse(text) as T;
    } catch (error) {
      clearTimeout(timeoutId);

      if (error instanceof Error) {
        if (error.name === 'AbortError') {
          throw createError(ErrorCode.TIMEOUT, 'Request timed out');
        }
        if ((error as ShadowMeshError).code) {
          throw error;
        }
      }

      throw createError(
        ErrorCode.NETWORK_ERROR,
        `Network error: ${error instanceof Error ? error.message : 'Unknown'}`
      );
    }
  }

  /**
   * GET request
   */
  async get<T>(path: string): Promise<T> {
    return this.request<T>('GET', path);
  }

  /**
   * POST request
   */
  async post<T>(path: string, body?: unknown): Promise<T> {
    return this.request<T>('POST', path, { body });
  }

  /**
   * PUT request
   */
  async put<T>(path: string, body?: unknown): Promise<T> {
    return this.request<T>('PUT', path, { body });
  }

  /**
   * DELETE request
   */
  async delete<T>(path: string): Promise<T> {
    return this.request<T>('DELETE', path);
  }

  // =========================================================================
  // Health & Status APIs
  // =========================================================================

  /**
   * Check gateway health
   */
  async health(): Promise<GatewayHealth> {
    return this.get<GatewayHealth>('/health');
  }

  /**
   * Get gateway metrics
   */
  async metrics(): Promise<GatewayMetrics> {
    return this.get<GatewayMetrics>('/metrics');
  }

  // =========================================================================
  // Content APIs
  // =========================================================================

  /**
   * Get content by CID
   */
  async getContent(cid: string): Promise<ArrayBuffer> {
    const response = await fetch(`${this.baseUrl}/ipfs/${cid}`, {
      headers: this.apiKey ? { Authorization: `Bearer ${this.apiKey}` } : {},
    });

    if (!response.ok) {
      if (response.status === 404) {
        throw createError(ErrorCode.CONTENT_NOT_FOUND, `Content not found: ${cid}`);
      }
      throw createError(
        ErrorCode.NETWORK_ERROR,
        `Failed to fetch content: ${response.status}`
      );
    }

    return response.arrayBuffer();
  }

  /**
   * Get content manifest
   */
  async getManifest(cid: string): Promise<ContentManifest> {
    return this.get<ContentManifest>(`/api/manifest/${cid}`);
  }

  /**
   * Check content status
   */
  async getStatus(cid: string): Promise<DeploymentStatus> {
    return this.get<DeploymentStatus>(`/api/status/${cid}`);
  }

  /**
   * Upload content
   */
  async uploadContent(
    data: Uint8Array,
    manifest: Partial<ContentManifest>
  ): Promise<{ cid: string; manifest: ContentManifest }> {
    // First upload the raw content
    const uploadResponse = await fetch(`${this.baseUrl}/api/upload`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/octet-stream',
        ...(this.apiKey ? { Authorization: `Bearer ${this.apiKey}` } : {}),
      },
      body: data as unknown as BodyInit,
    });

    if (!uploadResponse.ok) {
      throw createError(
        ErrorCode.UPLOAD_FAILED,
        `Upload failed: ${uploadResponse.status}`
      );
    }

    return uploadResponse.json();
  }

  /**
   * Announce content to the network
   */
  async announceContent(
    manifest: ContentManifest,
    options: {
      privacy?: string;
      redundancy?: number;
    } = {}
  ): Promise<void> {
    await this.post('/api/announce', {
      manifest,
      privacy: options.privacy || 'medium',
      redundancy: options.redundancy || 3,
    });
  }

  /**
   * Pin content
   */
  async pinContent(cid: string): Promise<void> {
    await this.post(`/api/pin/${cid}`);
  }

  /**
   * Unpin content
   */
  async unpinContent(cid: string): Promise<void> {
    await this.delete(`/api/pin/${cid}`);
  }

  // =========================================================================
  // Network APIs
  // =========================================================================

  /**
   * Get network statistics
   */
  async networkStats(): Promise<NetworkStats> {
    return this.get<NetworkStats>('/api/stats');
  }

  /**
   * Get node information
   */
  async nodeInfo(): Promise<NodeInfo> {
    return this.get<NodeInfo>('/api/node');
  }

  /**
   * Get list of known peers
   */
  async peers(): Promise<{ peers: Array<{ peerId: string; addresses: string[] }> }> {
    return this.get('/api/peers');
  }
}

/**
 * Node client for direct P2P communication
 */
export class NodeClient {
  private baseUrl: string;
  private timeout: number;
  private debug: boolean;

  constructor(config: ShadowMeshConfig = {}) {
    this.baseUrl = config.nodeUrl || 'http://localhost:3030';
    this.timeout = config.timeout || DEFAULT_TIMEOUT;
    this.debug = config.debug || false;
  }

  /**
   * Make a request to the node
   */
  private async request<T>(
    method: string,
    path: string,
    body?: unknown
  ): Promise<T> {
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
      throw createError(
        ErrorCode.NETWORK_ERROR,
        `Node request failed: ${response.status}`
      );
    }

    const text = await response.text();
    return text ? JSON.parse(text) : ({} as T);
  }

  /**
   * Get node status
   */
  async status(): Promise<{
    peerId: string;
    name: string;
    version: string;
    uptime: number;
    storageUsed: number;
    storageCapacity: number;
    connectedPeers: number;
  }> {
    return this.request('GET', '/api/status');
  }

  /**
   * Get node health
   */
  async health(): Promise<{
    healthy: boolean;
    checks: { storage: boolean; network: boolean; api: boolean };
  }> {
    return this.request('GET', '/api/health');
  }

  /**
   * Get node metrics
   */
  async metrics(): Promise<{
    uptime: number;
    requests: { total: number; successful: number; failed: number };
    bandwidth: { totalServed: number; totalReceived: number };
    network: { connectedPeers: number };
  }> {
    return this.request('GET', '/api/metrics');
  }

  /**
   * Get node configuration
   */
  async getConfig(): Promise<Record<string, unknown>> {
    return this.request('GET', '/api/config');
  }

  /**
   * Update node configuration
   */
  async updateConfig(config: {
    maxStorageGb?: number;
    maxBandwidthMbps?: number;
    maxPeers?: number;
    nodeName?: string;
  }): Promise<Record<string, unknown>> {
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
  async listContent(): Promise<Array<{
    cid: string;
    name: string;
    size: number;
    pinned: boolean;
    storedAt: number;
  }>> {
    return this.request('GET', '/api/storage/content');
  }

  /**
   * Pin content on node
   */
  async pin(cid: string): Promise<void> {
    await this.request('POST', `/api/storage/pin/${cid}`);
  }

  /**
   * Unpin content on node
   */
  async unpin(cid: string): Promise<void> {
    await this.request('POST', `/api/storage/unpin/${cid}`);
  }

  /**
   * Run garbage collection
   */
  async gc(targetFreeGb: number = 1): Promise<{
    freedBytes: number;
    removedFragments: number;
    removedContent: number;
  }> {
    return this.request('POST', '/api/storage/gc', { target_free_gb: targetFreeGb });
  }

  /**
   * Shutdown node gracefully
   */
  async shutdown(): Promise<void> {
    await this.request('POST', '/api/node/shutdown');
  }
}

/**
 * Create a gateway client
 */
export function createGatewayClient(config?: ShadowMeshConfig): GatewayClient {
  return new GatewayClient(config);
}

/**
 * Create a node client
 */
export function createNodeClient(config?: ShadowMeshConfig): NodeClient {
  return new NodeClient(config);
}
