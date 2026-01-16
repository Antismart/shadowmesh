/**
 * ShadowMesh SDK - API Client Module
 *
 * HTTP client for communicating with ShadowMesh gateway and nodes.
 */
import type { ShadowMeshConfig, ContentManifest, DeploymentStatus, NetworkStats, NodeInfo, GatewayHealth, GatewayMetrics } from './types.js';
/**
 * API Client for ShadowMesh gateway communication
 */
export declare class GatewayClient {
    private baseUrl;
    private apiKey?;
    private timeout;
    private debug;
    constructor(config?: ShadowMeshConfig);
    /**
     * Make an HTTP request
     */
    private request;
    /**
     * GET request
     */
    get<T>(path: string): Promise<T>;
    /**
     * POST request
     */
    post<T>(path: string, body?: unknown): Promise<T>;
    /**
     * PUT request
     */
    put<T>(path: string, body?: unknown): Promise<T>;
    /**
     * DELETE request
     */
    delete<T>(path: string): Promise<T>;
    /**
     * Check gateway health
     */
    health(): Promise<GatewayHealth>;
    /**
     * Get gateway metrics
     */
    metrics(): Promise<GatewayMetrics>;
    /**
     * Get content by CID
     */
    getContent(cid: string): Promise<ArrayBuffer>;
    /**
     * Get content manifest
     */
    getManifest(cid: string): Promise<ContentManifest>;
    /**
     * Check content status
     */
    getStatus(cid: string): Promise<DeploymentStatus>;
    /**
     * Upload content
     */
    uploadContent(data: Uint8Array, manifest: Partial<ContentManifest>): Promise<{
        cid: string;
        manifest: ContentManifest;
    }>;
    /**
     * Announce content to the network
     */
    announceContent(manifest: ContentManifest, options?: {
        privacy?: string;
        redundancy?: number;
    }): Promise<void>;
    /**
     * Pin content
     */
    pinContent(cid: string): Promise<void>;
    /**
     * Unpin content
     */
    unpinContent(cid: string): Promise<void>;
    /**
     * Get network statistics
     */
    networkStats(): Promise<NetworkStats>;
    /**
     * Get node information
     */
    nodeInfo(): Promise<NodeInfo>;
    /**
     * Get list of known peers
     */
    peers(): Promise<{
        peers: Array<{
            peerId: string;
            addresses: string[];
        }>;
    }>;
}
/**
 * Node client for direct P2P communication
 */
export declare class NodeClient {
    private baseUrl;
    private timeout;
    private debug;
    constructor(config?: ShadowMeshConfig);
    /**
     * Make a request to the node
     */
    private request;
    /**
     * Get node status
     */
    status(): Promise<{
        peerId: string;
        name: string;
        version: string;
        uptime: number;
        storageUsed: number;
        storageCapacity: number;
        connectedPeers: number;
    }>;
    /**
     * Get node health
     */
    health(): Promise<{
        healthy: boolean;
        checks: {
            storage: boolean;
            network: boolean;
            api: boolean;
        };
    }>;
    /**
     * Get node metrics
     */
    metrics(): Promise<{
        uptime: number;
        requests: {
            total: number;
            successful: number;
            failed: number;
        };
        bandwidth: {
            totalServed: number;
            totalReceived: number;
        };
        network: {
            connectedPeers: number;
        };
    }>;
    /**
     * Get node configuration
     */
    getConfig(): Promise<Record<string, unknown>>;
    /**
     * Update node configuration
     */
    updateConfig(config: {
        maxStorageGb?: number;
        maxBandwidthMbps?: number;
        maxPeers?: number;
        nodeName?: string;
    }): Promise<Record<string, unknown>>;
    /**
     * List stored content
     */
    listContent(): Promise<Array<{
        cid: string;
        name: string;
        size: number;
        pinned: boolean;
        storedAt: number;
    }>>;
    /**
     * Pin content on node
     */
    pin(cid: string): Promise<void>;
    /**
     * Unpin content on node
     */
    unpin(cid: string): Promise<void>;
    /**
     * Run garbage collection
     */
    gc(targetFreeGb?: number): Promise<{
        freedBytes: number;
        removedFragments: number;
        removedContent: number;
    }>;
    /**
     * Shutdown node gracefully
     */
    shutdown(): Promise<void>;
}
/**
 * Create a gateway client
 */
export declare function createGatewayClient(config?: ShadowMeshConfig): GatewayClient;
/**
 * Create a node client
 */
export declare function createNodeClient(config?: ShadowMeshConfig): NodeClient;
//# sourceMappingURL=client.d.ts.map