/**
 * ShadowMesh Decentralized Name Resolver
 *
 * Resolves `.shadow` names via the naming layer, eliminating centralized DNS.
 *
 * Resolution chain:
 * 1. Local cache (in-memory, TTL-based)
 * 2. Gateway naming API (`/api/names/:name`)
 * 3. Fallback to hardcoded bootstrap URLs
 */
/** A resolved name record from the naming layer */
export interface NameResolution {
    /** The resolved name */
    name: string;
    /** Resolution targets */
    records: NameRecordEntry[];
    /** Cache TTL in seconds */
    ttl: number;
    /** When this was resolved (unix ms) */
    resolvedAt: number;
    /** How this was resolved */
    source: 'cache' | 'gateway' | 'fallback';
}
/** A single resolution target within a name record */
export interface NameRecordEntry {
    type: 'content' | 'gateway' | 'service' | 'peer' | 'alias';
    cid?: string;
    peerId?: string;
    multiaddrs?: string[];
    serviceType?: string;
    weight?: number;
    target?: string;
}
/** Full name record as stored in DHT (matches Rust NameRecord) */
export interface NameRecord {
    name: string;
    name_hash: string;
    owner_pubkey: string;
    records: NameRecordEntry[];
    sequence: number;
    created_at: number;
    updated_at: number;
    ttl_seconds: number;
    signature: string;
}
/** Service registry entry (matches Rust ServiceRegistryEntry) */
export interface ServiceRegistryEntry {
    service_type: string;
    peer_id: string;
    multiaddrs: string[];
    reputation: number;
    registered_at: number;
    last_heartbeat: number;
    signature: string;
}
/** Official bootstrap multiaddrs (IP-only, no DNS) */
export declare const BOOTSTRAP_MULTIADDRS: string[];
/**
 * Fallback gateway URLs — used ONLY when naming resolution fails entirely.
 * These are the legacy centralized endpoints kept for backwards compatibility.
 */
export declare const FALLBACK_GATEWAY_URLS: {
    readonly mainnet: "https://api.shadowmesh.network";
    readonly testnet: "https://testnet.shadowmesh.network";
    readonly local: "http://localhost:3000";
};
/** Well-known service names */
export declare const WELL_KNOWN_NAMES: {
    readonly GATEWAY: "_gateway.shadow";
    readonly SIGNALING: "_signaling.shadow";
    readonly BOOTSTRAP: "_bootstrap.shadow";
    readonly TURN: "_turn.shadow";
    readonly STUN: "_stun.shadow";
};
export interface NameResolverConfig {
    /** Known gateway URLs to query for name resolution */
    gatewayUrls?: string[];
    /** Bootstrap multiaddrs for P2P discovery */
    bootstrapMultiaddrs?: string[];
    /** Enable ENS resolution for .eth names */
    enableEns?: boolean;
    /** Ethereum RPC URL for ENS */
    ensRpcUrl?: string;
    /** Request timeout in ms */
    timeout?: number;
}
export declare class NameResolver {
    private cache;
    private gatewayUrls;
    private timeout;
    constructor(config?: NameResolverConfig);
    /**
     * Resolve a `.shadow` name.
     *
     * Resolution chain: cache -> gateway API -> null
     */
    resolve(name: string): Promise<NameResolution | null>;
    /**
     * Resolve the best gateway URL from the naming layer.
     * Falls back to the provided fallback URL if resolution fails.
     */
    resolveGatewayUrl(fallback: string): Promise<string>;
    /**
     * Resolve signaling WebSocket URL from the naming layer.
     */
    resolveSignalingUrl(fallback: string): Promise<string>;
    /**
     * Discover services by type via the gateway's service discovery API.
     */
    resolveService(serviceType: string): Promise<ServiceRegistryEntry[]>;
    /**
     * Add a known gateway URL for resolution queries.
     */
    addGatewayUrl(url: string): void;
    /**
     * Invalidate a cached name.
     */
    invalidate(name: string): void;
    /**
     * Clear the entire cache.
     */
    clearCache(): void;
    /** Number of cached entries */
    get cacheSize(): number;
    private getFromCache;
    private setCache;
    private resolveViaGateway;
}
//# sourceMappingURL=resolver.d.ts.map