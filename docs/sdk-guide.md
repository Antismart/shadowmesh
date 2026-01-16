# ShadowMesh SDK Guide

The ShadowMesh SDK provides a TypeScript/JavaScript client library for interacting with the ShadowMesh decentralized CDN.

## Installation

```bash
npm install @shadowmesh/sdk
# or
yarn add @shadowmesh/sdk
# or
pnpm add @shadowmesh/sdk
```

## Quick Start

```typescript
import { ShadowMeshClient } from '@shadowmesh/sdk';

// Create client
const client = new ShadowMeshClient({
  gatewayUrl: 'http://localhost:3000'
});

// Deploy content
const content = new TextEncoder().encode('Hello, ShadowMesh!');
const { cid } = await client.deploy(content);

// Retrieve content
const retrieved = await client.retrieve(cid);
console.log(new TextDecoder().decode(retrieved));
```

## Configuration

### Client Options

```typescript
interface ClientConfig {
  // Gateway URL (required)
  gatewayUrl: string;
  
  // Request timeout in milliseconds (default: 30000)
  timeout?: number;
  
  // Number of retries for failed requests (default: 3)
  retries?: number;
  
  // Retry delay in milliseconds (default: 1000)
  retryDelay?: number;
  
  // Encryption settings
  encryption?: {
    enabled: boolean;      // Enable encryption (default: true)
    algorithm: string;     // 'ChaCha20-Poly1305' (default)
  };
  
  // Cache settings
  cache?: {
    enabled: boolean;      // Enable caching (default: true)
    maxSize: number;       // Max cache size in bytes
    ttl: number;           // TTL in milliseconds
  };
}
```

### Example Configuration

```typescript
const client = new ShadowMeshClient({
  gatewayUrl: 'https://gateway.shadowmesh.io',
  timeout: 60000,
  retries: 5,
  retryDelay: 2000,
  encryption: {
    enabled: true,
    algorithm: 'ChaCha20-Poly1305'
  },
  cache: {
    enabled: true,
    maxSize: 100 * 1024 * 1024, // 100MB
    ttl: 3600000 // 1 hour
  }
});
```

## Content Operations

### Deploying Content

#### Basic Deployment

```typescript
// Deploy raw bytes
const data = new Uint8Array([1, 2, 3, 4, 5]);
const result = await client.deploy(data);

// Deploy text
const text = new TextEncoder().encode('Hello, World!');
const result = await client.deploy(text);

// Deploy file (in browser)
const file = document.getElementById('file-input').files[0];
const buffer = await file.arrayBuffer();
const result = await client.deploy(new Uint8Array(buffer));
```

#### Deployment Options

```typescript
const result = await client.deploy(content, {
  // Encrypt content before upload
  encrypt: true,
  
  // Pin content (prevent garbage collection)
  pin: true,
  
  // Custom metadata
  metadata: {
    name: 'document.pdf',
    contentType: 'application/pdf',
    tags: ['important', 'work']
  },
  
  // Progress callback
  onProgress: (progress) => {
    console.log(`${progress.percent}% complete`);
  }
});

console.log(result.cid);      // Content ID
console.log(result.size);     // Size in bytes
console.log(result.key);      // Encryption key (if encrypted)
```

### Retrieving Content

#### Basic Retrieval

```typescript
// Retrieve by CID
const content = await client.retrieve('Qm...');

// Convert to string
const text = new TextDecoder().decode(content);

// Create blob URL (for images, videos, etc.)
const blob = new Blob([content]);
const url = URL.createObjectURL(blob);
```

#### Retrieval Options

```typescript
const content = await client.retrieve(cid, {
  // Decryption key (required if content was encrypted)
  key: encryptionKey,
  
  // Verify content hash
  verify: true,
  
  // Timeout for this request
  timeout: 60000,
  
  // Progress callback
  onProgress: (progress) => {
    console.log(`Downloaded: ${progress.bytesReceived} bytes`);
  }
});
```

### Content Status

```typescript
const status = await client.status(cid);

console.log(status.cid);              // Content ID
console.log(status.size);             // Size in bytes
console.log(status.fragments);        // Number of fragments
console.log(status.providers);        // Number of providers
console.log(status.replicationStatus); // 'healthy' | 'degraded' | 'critical'
console.log(status.pinned);           // Is content pinned
console.log(status.createdAt);        // Creation timestamp
```

### Pinning

```typescript
// Pin content
await client.pin(cid);

// Unpin content
await client.unpin(cid);
```

## Encryption

### Using CryptoProvider

```typescript
import { CryptoProvider } from '@shadowmesh/sdk';

const crypto = new CryptoProvider();

// Generate a new key
const key = await crypto.generateKey();

// Encrypt data
const plaintext = new TextEncoder().encode('Secret message');
const encrypted = await crypto.encrypt(plaintext, key);

// Decrypt data
const decrypted = await crypto.decrypt(encrypted, key);
console.log(new TextDecoder().decode(decrypted));
```

### End-to-End Encryption Flow

```typescript
const crypto = new CryptoProvider();
const client = new ShadowMeshClient({ gatewayUrl: '...' });

// 1. Generate encryption key
const key = await crypto.generateKey();

// 2. Encrypt content
const plaintext = new TextEncoder().encode('Sensitive data');
const encrypted = await crypto.encrypt(plaintext, key);

// 3. Upload encrypted content
const { cid } = await client.deploy(encrypted.ciphertext, {
  encrypt: false // Already encrypted
});

// 4. Share CID and key with recipient
// (use secure channel for key)

// 5. Recipient retrieves and decrypts
const ciphertext = await client.retrieve(cid);
const decrypted = await crypto.decrypt({
  ciphertext,
  nonce: encrypted.nonce
}, key);
```

### Key Management

```typescript
import { CryptoProvider, KeyDerivation } from '@shadowmesh/sdk';

// Derive key from password
const password = 'user-password';
const salt = crypto.getRandomValues(new Uint8Array(16));
const key = await KeyDerivation.deriveKey(password, salt);

// Export key for storage
const exported = await crypto.exportKey(key);
const keyString = btoa(String.fromCharCode(...exported));

// Import key
const imported = Uint8Array.from(atob(keyString), c => c.charCodeAt(0));
const restoredKey = await crypto.importKey(imported);
```

## Storage

### Memory Storage

For testing or temporary storage:

```typescript
import { MemoryStorage } from '@shadowmesh/sdk';

const storage = new MemoryStorage();

await storage.set('key', new Uint8Array([1, 2, 3]));
const data = await storage.get('key');
await storage.delete('key');
```

### IndexedDB Storage

For persistent browser storage:

```typescript
import { IndexedDBStorage } from '@shadowmesh/sdk';

const storage = new IndexedDBStorage('shadowmesh-app');

// Store content
await storage.set(cid, content);

// Retrieve content
const cached = await storage.get(cid);

// Check if content exists
const exists = await storage.has(cid);

// Get storage stats
const stats = await storage.stats();
console.log(`Used: ${stats.used} bytes`);
console.log(`Items: ${stats.count}`);
```

### IPFS Gateway Storage

For IPFS backend:

```typescript
import { IPFSGateway } from '@shadowmesh/sdk';

const ipfs = new IPFSGateway('https://ipfs.io');

// Fetch from IPFS
const content = await ipfs.cat(cid);
```

## Caching

### Content Cache

```typescript
import { ContentCache, MemoryStorage } from '@shadowmesh/sdk';

const cache = new ContentCache({
  maxSize: 50 * 1024 * 1024, // 50MB
  ttl: 30 * 60 * 1000,       // 30 minutes
  storage: new MemoryStorage()
});

// Cache content
await cache.set(cid, content);

// Get cached content (returns null if expired or not found)
const cached = await cache.get(cid);

// Invalidate specific entry
await cache.invalidate(cid);

// Clear entire cache
await cache.clear();

// Get cache stats
const stats = cache.getStats();
console.log(`Hit rate: ${stats.hitRate}%`);
```

### Request Deduplication

Prevent duplicate network requests:

```typescript
import { RequestDeduplicator } from '@shadowmesh/sdk';

const dedup = new RequestDeduplicator();

// Multiple calls to same CID will share one request
const [result1, result2] = await Promise.all([
  dedup.dedupe(cid, () => client.retrieve(cid)),
  dedup.dedupe(cid, () => client.retrieve(cid))
]);
```

### Content Preloader

Preload content that might be needed:

```typescript
import { ContentPreloader } from '@shadowmesh/sdk';

const preloader = new ContentPreloader({
  client,
  cache,
  maxConcurrent: 3
});

// Preload multiple CIDs
await preloader.preload([cid1, cid2, cid3]);

// Check preload status
const status = preloader.getStatus(cid1);
console.log(status.state); // 'pending' | 'loading' | 'loaded' | 'error'
```

## Utilities

### Formatting

```typescript
import { formatBytes, formatDuration, formatCid } from '@shadowmesh/sdk';

formatBytes(1024);           // "1.0 KB"
formatBytes(1073741824);     // "1.0 GB"

formatDuration(3661000);     // "1h 1m 1s"

formatCid('QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG');
// "QmYwAP...nPbdG" (truncated)
```

### Encoding

```typescript
import { hexEncode, hexDecode, base64Encode, base64Decode } from '@shadowmesh/sdk';

const bytes = new Uint8Array([1, 2, 3]);

// Hex
const hex = hexEncode(bytes);     // "010203"
const decoded = hexDecode(hex);   // Uint8Array([1, 2, 3])

// Base64
const b64 = base64Encode(bytes);  // "AQID"
const decoded2 = base64Decode(b64);
```

### Retry Logic

```typescript
import { retry } from '@shadowmesh/sdk';

const result = await retry(
  () => client.retrieve(cid),
  {
    retries: 3,
    delay: 1000,
    backoff: 2,            // Exponential backoff
    onRetry: (err, attempt) => {
      console.log(`Retry ${attempt}: ${err.message}`);
    }
  }
);
```

### Event Emitter

```typescript
import { EventEmitter } from '@shadowmesh/sdk';

const events = new EventEmitter<{
  'content:uploaded': { cid: string; size: number };
  'content:retrieved': { cid: string };
  'error': Error;
}>();

// Subscribe
const unsubscribe = events.on('content:uploaded', (data) => {
  console.log(`Uploaded: ${data.cid}`);
});

// Emit
events.emit('content:uploaded', { cid: 'Qm...', size: 1024 });

// Unsubscribe
unsubscribe();
```

## CLI Usage

The SDK includes a CLI for command-line operations:

```bash
# Install globally
npm install -g @shadowmesh/sdk

# Or use with npx
npx shadowmesh <command>
```

### Commands

```bash
# Initialize configuration
shadowmesh init

# Deploy content
shadowmesh deploy <file>
shadowmesh deploy ./document.pdf --pin

# Retrieve content
shadowmesh get <cid>
shadowmesh get <cid> --output ./output.pdf

# Check status
shadowmesh status <cid>

# Pin/unpin content
shadowmesh pin <cid>
shadowmesh unpin <cid>

# View statistics
shadowmesh stats

# Show configuration
shadowmesh config

# Show info
shadowmesh info
```

### Configuration File

Create `~/.shadowmesh/config.json`:

```json
{
  "gatewayUrl": "http://localhost:3000",
  "timeout": 30000,
  "retries": 3,
  "encryption": {
    "enabled": true
  }
}
```

## Error Handling

```typescript
import { ShadowMeshError, ErrorCode } from '@shadowmesh/sdk';

try {
  const content = await client.retrieve(cid);
} catch (error) {
  if (error instanceof ShadowMeshError) {
    switch (error.code) {
      case ErrorCode.NOT_FOUND:
        console.log('Content not found');
        break;
      case ErrorCode.NETWORK_ERROR:
        console.log('Network error:', error.message);
        break;
      case ErrorCode.TIMEOUT:
        console.log('Request timed out');
        break;
      case ErrorCode.HASH_MISMATCH:
        console.log('Content verification failed');
        break;
      default:
        console.log('Unknown error:', error.message);
    }
  }
}
```

## Browser Usage

### Script Tag

```html
<script src="https://unpkg.com/@shadowmesh/sdk/dist/browser.js"></script>
<script>
  const client = new ShadowMesh.Client({
    gatewayUrl: 'http://localhost:3000'
  });
  
  async function uploadFile(file) {
    const buffer = await file.arrayBuffer();
    const result = await client.deploy(new Uint8Array(buffer));
    console.log('Uploaded:', result.cid);
  }
</script>
```

### ES Modules

```html
<script type="module">
  import { ShadowMeshClient } from 'https://unpkg.com/@shadowmesh/sdk/dist/esm/index.js';
  
  const client = new ShadowMeshClient({
    gatewayUrl: 'http://localhost:3000'
  });
</script>
```

### React Example

```tsx
import { useState } from 'react';
import { ShadowMeshClient } from '@shadowmesh/sdk';

const client = new ShadowMeshClient({
  gatewayUrl: process.env.REACT_APP_GATEWAY_URL
});

function FileUploader() {
  const [cid, setCid] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const handleUpload = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    setLoading(true);
    try {
      const buffer = await file.arrayBuffer();
      const result = await client.deploy(new Uint8Array(buffer));
      setCid(result.cid);
    } catch (error) {
      console.error('Upload failed:', error);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div>
      <input type="file" onChange={handleUpload} disabled={loading} />
      {loading && <p>Uploading...</p>}
      {cid && <p>Uploaded: {cid}</p>}
    </div>
  );
}
```

## Node.js Usage

```typescript
import { ShadowMeshClient } from '@shadowmesh/sdk';
import { readFile, writeFile } from 'fs/promises';

const client = new ShadowMeshClient({
  gatewayUrl: 'http://localhost:3000'
});

// Upload file
const content = await readFile('./document.pdf');
const { cid } = await client.deploy(new Uint8Array(content));

// Download file
const retrieved = await client.retrieve(cid);
await writeFile('./downloaded.pdf', retrieved);
```

## TypeScript Support

The SDK is written in TypeScript and includes full type definitions:

```typescript
import type {
  ClientConfig,
  DeployOptions,
  DeployResult,
  RetrieveOptions,
  ContentStatus,
  NetworkStats,
  EncryptedData,
  StorageStats
} from '@shadowmesh/sdk';
```

## Testing

### Mocking the Client

```typescript
import { ShadowMeshClient } from '@shadowmesh/sdk';

// Create mock
const mockClient = {
  deploy: jest.fn().mockResolvedValue({ cid: 'Qm...', size: 100 }),
  retrieve: jest.fn().mockResolvedValue(new Uint8Array([1, 2, 3])),
  status: jest.fn().mockResolvedValue({ providers: 3, healthy: true })
};

// Use in tests
expect(mockClient.deploy).toHaveBeenCalledWith(expect.any(Uint8Array));
```

### Using Test Storage

```typescript
import { MemoryStorage } from '@shadowmesh/sdk';

// Use memory storage for tests
const storage = new MemoryStorage();

// Pre-populate with test data
await storage.set('test-cid', new Uint8Array([1, 2, 3]));
```
