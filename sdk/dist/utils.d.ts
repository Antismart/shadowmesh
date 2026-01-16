/**
 * ShadowMesh SDK - Utilities Module
 * Helper functions and utilities
 */
/**
 * Format bytes to human-readable string
 */
export declare function formatBytes(bytes: number, decimals?: number): string;
/**
 * Parse human-readable size string to bytes
 */
export declare function parseBytes(str: string): number;
/**
 * Format duration in milliseconds to human-readable string
 */
export declare function formatDuration(ms: number): string;
/**
 * Format timestamp to relative time (e.g., "2 hours ago")
 */
export declare function formatRelativeTime(timestamp: number | Date): string;
/**
 * Build URL with query parameters
 */
export declare function buildUrl(base: string, params?: Record<string, string | number | boolean | undefined>): string;
/**
 * Parse query parameters from URL
 */
export declare function parseQuery(url: string): Record<string, string>;
/**
 * Join URL paths safely
 */
export declare function joinPaths(...parts: string[]): string;
/**
 * Check if string is a valid CID
 */
export declare function isValidCid(cid: string): boolean;
/**
 * Extract CID from various URL formats
 */
export declare function extractCid(input: string): string | null;
/**
 * Build IPFS gateway URL
 */
export declare function buildIpfsUrl(gateway: string, cid: string, path?: string): string;
export interface RetryOptions {
    maxAttempts?: number;
    baseDelay?: number;
    maxDelay?: number;
    backoffFactor?: number;
    retryOn?: (error: Error) => boolean;
}
/**
 * Execute function with retry and exponential backoff
 */
export declare function retry<T>(fn: () => Promise<T>, options?: RetryOptions): Promise<T>;
/**
 * Sleep for specified milliseconds
 */
export declare function sleep(ms: number): Promise<void>;
/**
 * Create a timeout promise
 */
export declare function timeout<T>(promise: Promise<T>, ms: number, message?: string): Promise<T>;
/**
 * Execute promises with concurrency limit
 */
export declare function concurrent<T, R>(items: T[], fn: (item: T, index: number) => Promise<R>, concurrency: number): Promise<R[]>;
/**
 * Debounce function calls
 */
export declare function debounce<T extends (...args: Parameters<T>) => void>(fn: T, delay: number): (...args: Parameters<T>) => void;
/**
 * Throttle function calls
 */
export declare function throttle<T extends (...args: Parameters<T>) => void>(fn: T, limit: number): (...args: Parameters<T>) => void;
/**
 * Deep clone an object
 */
export declare function deepClone<T>(obj: T): T;
/**
 * Deep merge objects
 */
export declare function deepMerge<T extends Record<string, unknown>>(target: T, ...sources: Partial<T>[]): T;
/**
 * Convert Uint8Array to hex string
 */
export declare function bytesToHex(bytes: Uint8Array): string;
/**
 * Convert hex string to Uint8Array
 */
export declare function hexToBytes(hex: string): Uint8Array;
/**
 * Convert Uint8Array to base64 string
 */
export declare function bytesToBase64(bytes: Uint8Array): string;
/**
 * Convert base64 string to Uint8Array
 */
export declare function base64ToBytes(base64: string): Uint8Array;
/**
 * Convert string to Uint8Array (UTF-8)
 */
export declare function stringToBytes(str: string): Uint8Array;
/**
 * Convert Uint8Array to string (UTF-8)
 */
export declare function bytesToString(bytes: Uint8Array): string;
/**
 * Assert condition or throw error
 */
export declare function assert(condition: boolean, message: string): asserts condition;
/**
 * Ensure value is defined
 */
export declare function ensureDefined<T>(value: T | undefined | null, message?: string): T;
/**
 * Type guard for checking if value is an Error
 */
export declare function isError(value: unknown): value is Error;
/**
 * Check if running in browser environment
 */
export declare function isBrowser(): boolean;
/**
 * Check if running in Node.js environment
 */
export declare function isNode(): boolean;
/**
 * Check if running in a Web Worker
 */
export declare function isWebWorker(): boolean;
type EventListener<T> = (data: T) => void;
/**
 * Simple event emitter for SDK events
 */
export declare class EventEmitter<Events extends Record<string, unknown>> {
    private listeners;
    /**
     * Subscribe to an event
     */
    on<K extends keyof Events>(event: K, listener: EventListener<Events[K]>): () => void;
    /**
     * Subscribe to an event once
     */
    once<K extends keyof Events>(event: K, listener: EventListener<Events[K]>): () => void;
    /**
     * Unsubscribe from an event
     */
    off<K extends keyof Events>(event: K, listener: EventListener<Events[K]>): void;
    /**
     * Emit an event
     */
    emit<K extends keyof Events>(event: K, data: Events[K]): void;
    /**
     * Remove all listeners
     */
    removeAllListeners(event?: keyof Events): void;
}
export {};
//# sourceMappingURL=utils.d.ts.map