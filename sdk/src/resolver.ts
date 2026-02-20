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

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

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

interface CachedResolution {
  resolution: NameResolution;
  expiresAt: number;
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/** Official bootstrap multiaddrs (IP-only, no DNS) */
export const BOOTSTRAP_MULTIADDRS = [
  '/ip4/45.33.32.156/tcp/4001/p2p/12D3KooWBootstrapUSEast1placeholder',
  '/ip4/178.62.8.237/tcp/4001/p2p/12D3KooWBootstrapEUWest1placeholder',
  '/ip4/103.43.75.104/tcp/4001/p2p/12D3KooWBootstrapAPAC1placeholder',
];

/**
 * Fallback gateway URLs — used ONLY when naming resolution fails entirely.
 * These are the legacy centralized endpoints kept for backwards compatibility.
 */
export const FALLBACK_GATEWAY_URLS = {
  mainnet: 'https://api.shadowmesh.network',
  testnet: 'https://testnet.shadowmesh.network',
  local: 'http://localhost:3000',
} as const;

/** Well-known service names */
export const WELL_KNOWN_NAMES = {
  GATEWAY: '_gateway.shadow',
  SIGNALING: '_signaling.shadow',
  BOOTSTRAP: '_bootstrap.shadow',
  TURN: '_turn.shadow',
  STUN: '_stun.shadow',
} as const;

/** Default cache TTL in ms (5 minutes) */
const DEFAULT_CACHE_TTL_MS = 5 * 60 * 1000;

/** Maximum cache entries */
const MAX_CACHE_ENTRIES = 1000;

/** Resolver request timeout in ms */
const RESOLVE_TIMEOUT_MS = 10_000;

// ---------------------------------------------------------------------------
// NameResolver
// ---------------------------------------------------------------------------

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

export class NameResolver {
  private cache: Map<string, CachedResolution> = new Map();
  private gatewayUrls: string[];
  private timeout: number;

  constructor(config: NameResolverConfig = {}) {
    this.gatewayUrls = config.gatewayUrls || [];
    this.timeout = config.timeout || RESOLVE_TIMEOUT_MS;
  }

  /**
   * Resolve a `.shadow` name.
   *
   * Resolution chain: cache -> gateway API -> null
   */
  async resolve(name: string): Promise<NameResolution | null> {
    // 1. Check cache
    const cached = this.getFromCache(name);
    if (cached) return cached;

    // 2. Try gateway naming API
    for (const gatewayUrl of this.gatewayUrls) {
      try {
        const result = await this.resolveViaGateway(gatewayUrl, name);
        if (result) {
          this.setCache(name, result);
          return result;
        }
      } catch {
        // Try next gateway
        continue;
      }
    }

    return null;
  }

  /**
   * Resolve the best gateway URL from the naming layer.
   * Falls back to the provided fallback URL if resolution fails.
   */
  async resolveGatewayUrl(
    fallback: string,
  ): Promise<string> {
    try {
      const resolution = await this.resolve(WELL_KNOWN_NAMES.GATEWAY);
      if (resolution) {
        const gateways = resolution.records.filter(
          (r) => r.type === 'gateway' && r.multiaddrs && r.multiaddrs.length > 0,
        );
        if (gateways.length > 0) {
          // Return the first gateway's HTTP address
          // In practice, multiaddrs would be converted to HTTP URLs
          const bestGateway = gateways.sort(
            (a, b) => (b.weight || 0) - (a.weight || 0),
          )[0];
          // Extract IP from multiaddr if possible
          const addr = bestGateway.multiaddrs?.[0];
          if (addr) {
            const httpUrl = multiAddrToHttp(addr);
            if (httpUrl) return httpUrl;
          }
        }
      }
    } catch {
      // Fall through to fallback
    }

    return fallback;
  }

  /**
   * Resolve signaling WebSocket URL from the naming layer.
   */
  async resolveSignalingUrl(
    fallback: string,
  ): Promise<string> {
    try {
      const entries = await this.resolveService('signaling');
      if (entries.length > 0) {
        const addr = entries[0].multiaddrs?.[0];
        if (addr) {
          const wsUrl = multiAddrToWs(addr);
          if (wsUrl) return wsUrl;
        }
      }
    } catch {
      // Fall through
    }

    return fallback;
  }

  /**
   * Discover services by type via the gateway's service discovery API.
   */
  async resolveService(
    serviceType: string,
  ): Promise<ServiceRegistryEntry[]> {
    for (const gatewayUrl of this.gatewayUrls) {
      try {
        const controller = new AbortController();
        const timeoutId = setTimeout(() => controller.abort(), this.timeout);

        const response = await fetch(
          `${gatewayUrl}/api/services/${serviceType}`,
          { signal: controller.signal },
        );
        clearTimeout(timeoutId);

        if (response.ok) {
          const data = await response.json();
          return data.entries || [];
        }
      } catch {
        continue;
      }
    }

    return [];
  }

  /**
   * Add a known gateway URL for resolution queries.
   */
  addGatewayUrl(url: string): void {
    if (!this.gatewayUrls.includes(url)) {
      this.gatewayUrls.push(url);
    }
  }

  /**
   * Invalidate a cached name.
   */
  invalidate(name: string): void {
    this.cache.delete(name);
  }

  /**
   * Clear the entire cache.
   */
  clearCache(): void {
    this.cache.clear();
  }

  /** Number of cached entries */
  get cacheSize(): number {
    return this.cache.size;
  }

  // -- Private --

  private getFromCache(name: string): NameResolution | null {
    const entry = this.cache.get(name);
    if (!entry) return null;

    if (Date.now() > entry.expiresAt) {
      this.cache.delete(name);
      return null;
    }

    return { ...entry.resolution, source: 'cache' };
  }

  private setCache(name: string, resolution: NameResolution): void {
    // Evict if over capacity
    if (this.cache.size >= MAX_CACHE_ENTRIES) {
      // Delete oldest entry
      const oldestKey = this.cache.keys().next().value;
      if (oldestKey !== undefined) {
        this.cache.delete(oldestKey);
      }
    }

    const ttlMs = (resolution.ttl || 3600) * 1000;
    this.cache.set(name, {
      resolution,
      expiresAt: Date.now() + Math.min(ttlMs, DEFAULT_CACHE_TTL_MS),
    });
  }

  private async resolveViaGateway(
    gatewayUrl: string,
    name: string,
  ): Promise<NameResolution | null> {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), this.timeout);

    try {
      const response = await fetch(
        `${gatewayUrl}/api/names/${encodeURIComponent(name)}`,
        { signal: controller.signal },
      );
      clearTimeout(timeoutId);

      if (!response.ok) return null;

      const data = await response.json();
      if (!data.found || !data.record) return null;

      const record: NameRecord = data.record;
      return {
        name: record.name,
        records: record.records,
        ttl: record.ttl_seconds,
        resolvedAt: Date.now(),
        source: 'gateway',
      };
    } catch {
      clearTimeout(timeoutId);
      return null;
    }
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Convert a libp2p multiaddr to an HTTP URL (best effort).
 * e.g., `/ip4/1.2.3.4/tcp/3000` -> `http://1.2.3.4:3000`
 */
function multiAddrToHttp(addr: string): string | null {
  const ipMatch = addr.match(/\/ip[46]\/([^/]+)\/tcp\/(\d+)/);
  if (ipMatch) {
    const [, ip, port] = ipMatch;
    return `http://${ip}:${port}`;
  }
  return null;
}

/**
 * Convert a libp2p multiaddr to a WebSocket URL (best effort).
 * e.g., `/ip4/1.2.3.4/tcp/3000` -> `ws://1.2.3.4:3000/signaling/ws`
 */
function multiAddrToWs(addr: string): string | null {
  const ipMatch = addr.match(/\/ip[46]\/([^/]+)\/tcp\/(\d+)/);
  if (ipMatch) {
    const [, ip, port] = ipMatch;
    return `ws://${ip}:${port}/signaling/ws`;
  }
  return null;
}
