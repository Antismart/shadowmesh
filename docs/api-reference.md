# ShadowMesh API Reference

## Gateway HTTP API

Base URL: `http://localhost:3000`

### Endpoints

---

### `GET /`

Returns the gateway landing page.

**Response:**
- `200 OK` - HTML landing page

---

### `GET /health`

Health check endpoint.

**Response:**
```json
{
  "status": "healthy",
  "uptime_seconds": 3600,
  "version": "0.1.0"
}
```

**Status Codes:**
- `200 OK` - Gateway is healthy
- `503 Service Unavailable` - Gateway is unhealthy

---

### `GET /metrics`

Prometheus-compatible metrics endpoint.

**Response:**
```json
{
  "requests_total": 1000,
  "requests_success": 950,
  "requests_error": 50,
  "bytes_served": 104857600,
  "cache_hits": 800,
  "cache_misses": 200,
  "uptime_seconds": 3600
}
```

---

### `GET /:cid`

Retrieve content by Content ID.

**Parameters:**
| Name | Type | Description |
|------|------|-------------|
| `cid` | path | Content identifier (BLAKE3 hash) |

**Headers:**
| Name | Description |
|------|-------------|
| `Range` | Optional byte range for partial content |

**Response:**
- `200 OK` - Content bytes
- `206 Partial Content` - Partial content (with Range header)
- `404 Not Found` - Content not found
- `504 Gateway Timeout` - Content retrieval timed out

**Response Headers:**
```
Content-Type: application/octet-stream
Content-Length: 1024
X-Content-Hash: abc123...
```

**Example:**
```bash
curl http://localhost:3000/QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG
```

---

### `POST /upload`

Upload content to the network.

**Request:**
- `Content-Type: application/octet-stream` or `multipart/form-data`
- Body: Raw content bytes or file upload

**Response:**
```json
{
  "cid": "QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG",
  "size": 1024,
  "fragments": 4,
  "status": "uploaded"
}
```

**Status Codes:**
- `201 Created` - Content uploaded successfully
- `400 Bad Request` - Invalid content
- `413 Payload Too Large` - Content exceeds size limit
- `500 Internal Server Error` - Upload failed

**Example:**
```bash
curl -X POST \
  -H "Content-Type: application/octet-stream" \
  --data-binary @myfile.txt \
  http://localhost:3000/upload
```

---

### `GET /status/:cid`

Get status of content in the network.

**Parameters:**
| Name | Type | Description |
|------|------|-------------|
| `cid` | path | Content identifier |

**Response:**
```json
{
  "cid": "QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG",
  "size": 1024,
  "fragments": 4,
  "providers": 3,
  "replication_status": "healthy",
  "created_at": "2024-01-15T10:30:00Z"
}
```

**Status Codes:**
- `200 OK` - Status retrieved
- `404 Not Found` - Content not found

---

### `POST /pin/:cid`

Pin content to prevent garbage collection.

**Parameters:**
| Name | Type | Description |
|------|------|-------------|
| `cid` | path | Content identifier to pin |

**Response:**
```json
{
  "cid": "QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG",
  "pinned": true
}
```

---

### `DELETE /pin/:cid`

Unpin content.

**Parameters:**
| Name | Type | Description |
|------|------|-------------|
| `cid` | path | Content identifier to unpin |

**Response:**
```json
{
  "cid": "QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG",
  "pinned": false
}
```

---

## Node Runner API

Base URL: `http://localhost:8080`

### `GET /api/status`

Get node status.

**Response:**
```json
{
  "node_id": "12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp",
  "uptime_seconds": 7200,
  "peers_connected": 15,
  "content_count": 42,
  "storage_used_bytes": 1073741824,
  "storage_max_bytes": 10737418240
}
```

### `GET /api/peers`

List connected peers.

**Response:**
```json
{
  "peers": [
    {
      "peer_id": "12D3KooW...",
      "addresses": ["/ip4/192.168.1.100/tcp/4001"],
      "state": "connected",
      "score": 85.5,
      "latency_ms": 45
    }
  ],
  "total": 15
}
```

### `GET /api/content`

List stored content.

**Response:**
```json
{
  "content": [
    {
      "cid": "Qm...",
      "size": 1024,
      "pinned": true,
      "replicas": 3,
      "created_at": "2024-01-15T10:30:00Z"
    }
  ],
  "total": 42
}
```

### `GET /api/bandwidth`

Get bandwidth statistics.

**Response:**
```json
{
  "inbound": {
    "total_bytes": 1073741824,
    "rate_bps": 1048576
  },
  "outbound": {
    "total_bytes": 2147483648,
    "rate_bps": 2097152
  },
  "by_peer": [
    {
      "peer_id": "12D3KooW...",
      "inbound_bytes": 104857600,
      "outbound_bytes": 209715200
    }
  ]
}
```

---

## SDK API (TypeScript)

### ShadowMeshClient

Main client for interacting with ShadowMesh.

```typescript
import { ShadowMeshClient, ClientConfig } from '@shadowmesh/sdk';

const config: ClientConfig = {
  gatewayUrl: 'http://localhost:3000',
  timeout: 30000,
  retries: 3,
  encryption: {
    enabled: true,
    algorithm: 'ChaCha20-Poly1305'
  }
};

const client = new ShadowMeshClient(config);
```

#### Methods

##### `deploy(content: Uint8Array, options?: DeployOptions): Promise<DeployResult>`

Deploy content to the network.

```typescript
const content = new TextEncoder().encode('Hello, World!');
const result = await client.deploy(content, {
  encrypt: true,
  pin: true,
  metadata: { name: 'greeting.txt' }
});

console.log(result.cid);  // Content ID
console.log(result.size); // Size in bytes
```

##### `retrieve(cid: string, options?: RetrieveOptions): Promise<Uint8Array>`

Retrieve content by CID.

```typescript
const content = await client.retrieve('Qm...', {
  decrypt: true,
  verify: true
});

const text = new TextDecoder().decode(content);
```

##### `status(cid: string): Promise<ContentStatus>`

Get content status.

```typescript
const status = await client.status('Qm...');
console.log(status.providers);  // Number of providers
console.log(status.healthy);    // Replication health
```

##### `pin(cid: string): Promise<void>`

Pin content.

```typescript
await client.pin('Qm...');
```

##### `unpin(cid: string): Promise<void>`

Unpin content.

```typescript
await client.unpin('Qm...');
```

##### `stats(): Promise<NetworkStats>`

Get network statistics.

```typescript
const stats = await client.stats();
console.log(stats.totalRequests);
console.log(stats.bytesTransferred);
```

---

### CryptoProvider

Browser-compatible cryptographic operations.

```typescript
import { CryptoProvider } from '@shadowmesh/sdk';

const crypto = new CryptoProvider();
```

#### Methods

##### `generateKey(): Promise<CryptoKey>`

Generate a new encryption key.

```typescript
const key = await crypto.generateKey();
```

##### `encrypt(data: Uint8Array, key: CryptoKey): Promise<EncryptedData>`

Encrypt data.

```typescript
const encrypted = await crypto.encrypt(data, key);
```

##### `decrypt(encrypted: EncryptedData, key: CryptoKey): Promise<Uint8Array>`

Decrypt data.

```typescript
const decrypted = await crypto.decrypt(encrypted, key);
```

##### `hash(data: Uint8Array): Promise<string>`

Hash content.

```typescript
const hash = await crypto.hash(data);
```

---

### ContentStorage

Storage backends for persisting content.

```typescript
import { MemoryStorage, IndexedDBStorage } from '@shadowmesh/sdk';

// In-memory storage (for testing)
const memoryStorage = new MemoryStorage();

// IndexedDB storage (for browser)
const indexedStorage = new IndexedDBStorage('shadowmesh-cache');
```

#### Methods

##### `get(key: string): Promise<Uint8Array | null>`

Get content by key.

##### `set(key: string, value: Uint8Array): Promise<void>`

Store content.

##### `delete(key: string): Promise<void>`

Delete content.

##### `has(key: string): Promise<boolean>`

Check if content exists.

##### `clear(): Promise<void>`

Clear all content.

##### `stats(): Promise<StorageStats>`

Get storage statistics.

---

### ContentCache

LRU cache with TTL support.

```typescript
import { ContentCache } from '@shadowmesh/sdk';

const cache = new ContentCache({
  maxSize: 100 * 1024 * 1024, // 100MB
  ttl: 3600000,               // 1 hour
  storage: new MemoryStorage()
});
```

#### Methods

##### `get(key: string): Promise<Uint8Array | null>`

Get cached content.

##### `set(key: string, value: Uint8Array, ttl?: number): Promise<void>`

Cache content.

##### `invalidate(key: string): Promise<void>`

Invalidate cache entry.

##### `clear(): Promise<void>`

Clear entire cache.

---

## Error Codes

| Code | Name | Description |
|------|------|-------------|
| `1001` | `NETWORK_ERROR` | Network request failed |
| `1002` | `TIMEOUT` | Request timed out |
| `1003` | `NOT_FOUND` | Content not found |
| `1004` | `INVALID_CID` | Invalid content identifier |
| `2001` | `ENCRYPTION_FAILED` | Encryption operation failed |
| `2002` | `DECRYPTION_FAILED` | Decryption operation failed |
| `2003` | `HASH_MISMATCH` | Content hash verification failed |
| `3001` | `STORAGE_FULL` | Storage quota exceeded |
| `3002` | `STORAGE_ERROR` | Storage operation failed |

---

## Rate Limits

Default rate limits (configurable):

| Endpoint | Limit |
|----------|-------|
| `GET /:cid` | 100 req/min |
| `POST /upload` | 10 req/min |
| `GET /status/:cid` | 60 req/min |

Rate limit headers:
```
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 95
X-RateLimit-Reset: 1705312800
```

---

## Content Types

Supported content types with automatic detection:

| Type | Extension | MIME Type |
|------|-----------|-----------|
| Text | `.txt` | `text/plain` |
| HTML | `.html` | `text/html` |
| JSON | `.json` | `application/json` |
| Image | `.png, .jpg` | `image/*` |
| Video | `.mp4, .webm` | `video/*` |
| Binary | `*` | `application/octet-stream` |
