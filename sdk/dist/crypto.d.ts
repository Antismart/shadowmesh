/**
 * ShadowMesh SDK - Cryptography Module
 *
 * Client-side encryption, decryption, and hashing utilities.
 */
/**
 * Hash content using Blake3
 * Falls back to SHA-256 if Blake3 is not available
 */
export declare function hashContent(data: Uint8Array): Promise<string>;
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