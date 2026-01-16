/**
 * ShadowMesh SDK - Utilities Module
 * Helper functions and utilities
 */
// ============================================================================
// Size Formatting
// ============================================================================
const SIZE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
/**
 * Format bytes to human-readable string
 */
export function formatBytes(bytes, decimals = 2) {
    if (bytes === 0)
        return '0 B';
    if (bytes < 0)
        return '-' + formatBytes(-bytes, decimals);
    const k = 1024;
    const dm = Math.max(0, decimals);
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    const unit = SIZE_UNITS[Math.min(i, SIZE_UNITS.length - 1)];
    return `${parseFloat((bytes / Math.pow(k, i)).toFixed(dm))} ${unit}`;
}
/**
 * Parse human-readable size string to bytes
 */
export function parseBytes(str) {
    const match = str.match(/^([\d.]+)\s*([A-Za-z]+)?$/);
    if (!match) {
        throw new Error(`Invalid size format: ${str}`);
    }
    const value = parseFloat(match[1]);
    const unit = (match[2] || 'B').toUpperCase();
    const index = SIZE_UNITS.findIndex((u) => u === unit);
    if (index === -1) {
        throw new Error(`Unknown size unit: ${unit}`);
    }
    return Math.round(value * Math.pow(1024, index));
}
// ============================================================================
// Time Formatting
// ============================================================================
/**
 * Format duration in milliseconds to human-readable string
 */
export function formatDuration(ms) {
    if (ms < 0)
        return '-' + formatDuration(-ms);
    if (ms < 1000)
        return `${ms}ms`;
    const seconds = Math.floor(ms / 1000);
    if (seconds < 60)
        return `${seconds}s`;
    const minutes = Math.floor(seconds / 60);
    const remainingSeconds = seconds % 60;
    if (minutes < 60) {
        return remainingSeconds > 0 ? `${minutes}m ${remainingSeconds}s` : `${minutes}m`;
    }
    const hours = Math.floor(minutes / 60);
    const remainingMinutes = minutes % 60;
    if (hours < 24) {
        return remainingMinutes > 0 ? `${hours}h ${remainingMinutes}m` : `${hours}h`;
    }
    const days = Math.floor(hours / 24);
    const remainingHours = hours % 24;
    return remainingHours > 0 ? `${days}d ${remainingHours}h` : `${days}d`;
}
/**
 * Format timestamp to relative time (e.g., "2 hours ago")
 */
export function formatRelativeTime(timestamp) {
    const now = Date.now();
    const time = timestamp instanceof Date ? timestamp.getTime() : timestamp;
    const diff = now - time;
    if (diff < 0) {
        return 'in ' + formatDuration(-diff);
    }
    if (diff < 1000)
        return 'just now';
    if (diff < 60000)
        return `${Math.floor(diff / 1000)} seconds ago`;
    if (diff < 3600000)
        return `${Math.floor(diff / 60000)} minutes ago`;
    if (diff < 86400000)
        return `${Math.floor(diff / 3600000)} hours ago`;
    if (diff < 604800000)
        return `${Math.floor(diff / 86400000)} days ago`;
    if (diff < 2592000000)
        return `${Math.floor(diff / 604800000)} weeks ago`;
    if (diff < 31536000000)
        return `${Math.floor(diff / 2592000000)} months ago`;
    return `${Math.floor(diff / 31536000000)} years ago`;
}
// ============================================================================
// URL Utilities
// ============================================================================
/**
 * Build URL with query parameters
 */
export function buildUrl(base, params) {
    if (!params)
        return base;
    const url = new URL(base);
    for (const [key, value] of Object.entries(params)) {
        if (value !== undefined) {
            url.searchParams.set(key, String(value));
        }
    }
    return url.toString();
}
/**
 * Parse query parameters from URL
 */
export function parseQuery(url) {
    const parsed = new URL(url);
    const params = {};
    parsed.searchParams.forEach((value, key) => {
        params[key] = value;
    });
    return params;
}
/**
 * Join URL paths safely
 */
export function joinPaths(...parts) {
    return parts
        .map((part, i) => {
        if (i === 0)
            return part.replace(/\/+$/, '');
        if (i === parts.length - 1)
            return part.replace(/^\/+/, '');
        return part.replace(/^\/+/, '').replace(/\/+$/, '');
    })
        .filter(Boolean)
        .join('/');
}
// ============================================================================
// Content ID Utilities
// ============================================================================
const CID_REGEX = /^(Qm[1-9A-HJ-NP-Za-km-z]{44}|b[A-Za-z2-7]{58,})$/;
/**
 * Check if string is a valid CID
 */
export function isValidCid(cid) {
    return CID_REGEX.test(cid);
}
/**
 * Extract CID from various URL formats
 */
export function extractCid(input) {
    // Direct CID
    if (isValidCid(input))
        return input;
    // IPFS URL format: ipfs://Qm...
    if (input.startsWith('ipfs://')) {
        const cid = input.slice(7).split('/')[0];
        return isValidCid(cid) ? cid : null;
    }
    // Gateway URL format: https://gateway.example/ipfs/Qm...
    const gatewayMatch = input.match(/\/ipfs\/([^/?#]+)/);
    if (gatewayMatch && isValidCid(gatewayMatch[1])) {
        return gatewayMatch[1];
    }
    return null;
}
/**
 * Build IPFS gateway URL
 */
export function buildIpfsUrl(gateway, cid, path) {
    const base = gateway.replace(/\/$/, '');
    const url = `${base}/ipfs/${cid}`;
    return path ? joinPaths(url, path) : url;
}
/**
 * Execute function with retry and exponential backoff
 */
export async function retry(fn, options = {}) {
    const { maxAttempts = 3, baseDelay = 1000, maxDelay = 30000, backoffFactor = 2, retryOn = () => true, } = options;
    let lastError;
    for (let attempt = 1; attempt <= maxAttempts; attempt++) {
        try {
            return await fn();
        }
        catch (err) {
            lastError = err instanceof Error ? err : new Error(String(err));
            if (attempt === maxAttempts || !retryOn(lastError)) {
                throw lastError;
            }
            // Calculate delay with exponential backoff
            const delay = Math.min(baseDelay * Math.pow(backoffFactor, attempt - 1), maxDelay);
            // Add jitter (±10%)
            const jitter = delay * 0.1 * (Math.random() * 2 - 1);
            await sleep(delay + jitter);
        }
    }
    throw lastError ?? new Error('Retry failed');
}
// ============================================================================
// Async Utilities
// ============================================================================
/**
 * Sleep for specified milliseconds
 */
export function sleep(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}
/**
 * Create a timeout promise
 */
export function timeout(promise, ms, message) {
    return Promise.race([
        promise,
        sleep(ms).then(() => {
            throw new Error(message ?? `Timeout after ${ms}ms`);
        }),
    ]);
}
/**
 * Execute promises with concurrency limit
 */
export async function concurrent(items, fn, concurrency) {
    const results = new Array(items.length);
    let index = 0;
    const worker = async () => {
        while (index < items.length) {
            const i = index++;
            results[i] = await fn(items[i], i);
        }
    };
    const workers = Array.from({ length: Math.min(concurrency, items.length) }, worker);
    await Promise.all(workers);
    return results;
}
/**
 * Debounce function calls
 */
export function debounce(fn, delay) {
    let timeoutId = null;
    return (...args) => {
        if (timeoutId)
            clearTimeout(timeoutId);
        timeoutId = setTimeout(() => fn(...args), delay);
    };
}
/**
 * Throttle function calls
 */
export function throttle(fn, limit) {
    let lastCall = 0;
    return (...args) => {
        const now = Date.now();
        if (now - lastCall >= limit) {
            lastCall = now;
            fn(...args);
        }
    };
}
// ============================================================================
// Data Utilities
// ============================================================================
/**
 * Deep clone an object
 */
export function deepClone(obj) {
    if (obj === null || typeof obj !== 'object')
        return obj;
    if (obj instanceof Date)
        return new Date(obj.getTime());
    if (obj instanceof Uint8Array)
        return new Uint8Array(obj);
    if (Array.isArray(obj))
        return obj.map(deepClone);
    const cloned = {};
    for (const key of Object.keys(obj)) {
        cloned[key] = deepClone(obj[key]);
    }
    return cloned;
}
/**
 * Deep merge objects
 */
export function deepMerge(target, ...sources) {
    if (!sources.length)
        return target;
    const source = sources.shift();
    if (!source)
        return target;
    for (const key of Object.keys(source)) {
        const sourceValue = source[key];
        const targetValue = target[key];
        if (isPlainObject(sourceValue) && isPlainObject(targetValue)) {
            target[key] = deepMerge(targetValue, sourceValue);
        }
        else if (sourceValue !== undefined) {
            target[key] = sourceValue;
        }
    }
    return deepMerge(target, ...sources);
}
function isPlainObject(obj) {
    return typeof obj === 'object' && obj !== null && obj.constructor === Object;
}
// ============================================================================
// Encoding Utilities
// ============================================================================
/**
 * Convert Uint8Array to hex string
 */
export function bytesToHex(bytes) {
    return Array.from(bytes)
        .map((b) => b.toString(16).padStart(2, '0'))
        .join('');
}
/**
 * Convert hex string to Uint8Array
 */
export function hexToBytes(hex) {
    const clean = hex.replace(/^0x/, '');
    if (clean.length % 2 !== 0) {
        throw new Error('Invalid hex string');
    }
    const bytes = new Uint8Array(clean.length / 2);
    for (let i = 0; i < bytes.length; i++) {
        bytes[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
    }
    return bytes;
}
/**
 * Convert Uint8Array to base64 string
 */
export function bytesToBase64(bytes) {
    const binary = Array.from(bytes)
        .map((b) => String.fromCharCode(b))
        .join('');
    return btoa(binary);
}
/**
 * Convert base64 string to Uint8Array
 */
export function base64ToBytes(base64) {
    const binary = atob(base64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
        bytes[i] = binary.charCodeAt(i);
    }
    return bytes;
}
/**
 * Convert string to Uint8Array (UTF-8)
 */
export function stringToBytes(str) {
    return new TextEncoder().encode(str);
}
/**
 * Convert Uint8Array to string (UTF-8)
 */
export function bytesToString(bytes) {
    return new TextDecoder().decode(bytes);
}
// ============================================================================
// Validation Utilities
// ============================================================================
/**
 * Assert condition or throw error
 */
export function assert(condition, message) {
    if (!condition) {
        throw new Error(message);
    }
}
/**
 * Ensure value is defined
 */
export function ensureDefined(value, message) {
    if (value === undefined || value === null) {
        throw new Error(message ?? 'Value is undefined');
    }
    return value;
}
/**
 * Type guard for checking if value is an Error
 */
export function isError(value) {
    return value instanceof Error;
}
// ============================================================================
// Environment Utilities
// ============================================================================
/**
 * Check if running in browser environment
 */
export function isBrowser() {
    return typeof window !== 'undefined' && typeof document !== 'undefined';
}
/**
 * Check if running in Node.js environment
 */
export function isNode() {
    return (typeof globalThis !== 'undefined' &&
        typeof globalThis.process !== 'undefined' &&
        globalThis.process?.versions?.node != null);
}
/**
 * Check if running in a Web Worker
 */
export function isWebWorker() {
    return (typeof self !== 'undefined' &&
        typeof self.WorkerGlobalScope !== 'undefined');
}
/**
 * Simple event emitter for SDK events
 */
export class EventEmitter {
    listeners = new Map();
    /**
     * Subscribe to an event
     */
    on(event, listener) {
        if (!this.listeners.has(event)) {
            this.listeners.set(event, new Set());
        }
        this.listeners.get(event).add(listener);
        // Return unsubscribe function
        return () => this.off(event, listener);
    }
    /**
     * Subscribe to an event once
     */
    once(event, listener) {
        const wrapper = (data) => {
            this.off(event, wrapper);
            listener(data);
        };
        return this.on(event, wrapper);
    }
    /**
     * Unsubscribe from an event
     */
    off(event, listener) {
        this.listeners.get(event)?.delete(listener);
    }
    /**
     * Emit an event
     */
    emit(event, data) {
        this.listeners.get(event)?.forEach((listener) => {
            try {
                listener(data);
            }
            catch (err) {
                console.error(`Error in event listener for ${String(event)}:`, err);
            }
        });
    }
    /**
     * Remove all listeners
     */
    removeAllListeners(event) {
        if (event) {
            this.listeners.delete(event);
        }
        else {
            this.listeners.clear();
        }
    }
}
//# sourceMappingURL=utils.js.map