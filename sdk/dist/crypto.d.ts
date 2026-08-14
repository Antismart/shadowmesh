/**
 * ShadowMesh SDK - Cryptography Module
 *
 * Client-side encryption, decryption, and hashing utilities.
 */
/**
 * Hash content using BLAKE3, returning a bare lowercase hex string.
 *
 * This is the canonical content-hash format for the ShadowMesh fragment
 * protocol (matches `blake3::hash(..).to_hex()` on the Rust side). There is
 * intentionally NO SHA-256 fallback: silently substituting a different hash
 * algorithm would produce identifiers that neither the network nor the WASM
 * SDK could verify. If BLAKE3 is unavailable we fail loudly.
 */
export declare function hashContent(data: Uint8Array): Promise<string>;
/**
 * True when `id` is a bare hex BLAKE3 content hash (64 hex chars).
 *
 * The ShadowMesh fragment protocol identifies content by bare-hex BLAKE3.
 * IPFS-style CIDs (`Qm...` / `bafy...`) are a separate identifier class that
 * cannot be recomputed from bytes with BLAKE3 and are validated structurally
 * (see the `cid_validation` layer) instead of by content hash.
 */
export declare function isHexBlake3(id: string): boolean;
/**
 * Hash a string
 */
export declare function hashString(str: string): Promise<string>;
/**
 * Derive encryption key from password using PBKDF2
 */
export declare function deriveKey(password: string, salt: Uint8Array): Promise<CryptoKey>;
/**
 * Generate random bytes
 */
export declare function randomBytes(length: number): Uint8Array;
/**
 * Encryption result
 */
export interface EncryptedData {
    /** Encrypted ciphertext */
    ciphertext: Uint8Array;
    /** Initialization vector */
    iv: Uint8Array;
    /** Salt used for key derivation */
    salt: Uint8Array;
    /** Algorithm identifier */
    algorithm: string;
}
/**
 * Encrypt data with password
 */
export declare function encrypt(data: Uint8Array, password: string): Promise<EncryptedData>;
/**
 * Decrypt data with password
 */
export declare function decrypt(encrypted: EncryptedData, password: string): Promise<Uint8Array>;
/**
 * Serialize encrypted data for storage/transmission
 */
export declare function serializeEncrypted(encrypted: EncryptedData): Uint8Array;
/**
 * Deserialize encrypted data
 */
export declare function deserializeEncrypted(data: Uint8Array): EncryptedData;
/**
 * Verify content hash
 */
export declare function verifyHash(data: Uint8Array, expectedHash: string): Promise<boolean>;
/**
 * Generate a secure random ID
 */
export declare function generateId(length?: number): string;
/**
 * Constant-time string comparison to prevent timing attacks
 */
export declare function secureCompare(a: string, b: string): boolean;
/**
 * Encrypt content and return as base64
 */
export declare function encryptToBase64(data: Uint8Array, password: string): Promise<string>;
/**
 * Decrypt content from base64
 */
export declare function decryptFromBase64(base64Data: string, password: string): Promise<Uint8Array>;
//# sourceMappingURL=crypto.d.ts.map