/**
 * ShadowMesh SDK - Cryptography Module
 *
 * Client-side encryption, decryption, and hashing utilities.
 */
// We'll use Web Crypto API for browser compatibility
// and native crypto for Node.js
/**
 * Hash content using Blake3
 * Falls back to SHA-256 if Blake3 is not available
 */
export async function hashContent(data) {
    try {
        // Try to use blake3-wasm if available
        const blake3 = await import('blake3-wasm');
        const hash = blake3.hash(data);
        return typeof hash === 'string' ? hash : Buffer.from(hash).toString('hex');
    }
    catch {
        // Fallback to SHA-256
        const hashBuffer = await crypto.subtle.digest('SHA-256', data.buffer);
        return Buffer.from(hashBuffer).toString('hex');
    }
}
/**
 * Hash a string
 */
export async function hashString(str) {
    const encoder = new TextEncoder();
    return hashContent(encoder.encode(str));
}
/**
 * Derive encryption key from password using PBKDF2
 */
export async function deriveKey(password, salt) {
    const encoder = new TextEncoder();
    const passwordBuffer = encoder.encode(password);
    // Import password as key material
    const keyMaterial = await crypto.subtle.importKey('raw', passwordBuffer, 'PBKDF2', false, ['deriveKey']);
    // Derive AES-GCM key
    return crypto.subtle.deriveKey({
        name: 'PBKDF2',
        salt: salt.buffer,
        iterations: 100000,
        hash: 'SHA-256',
    }, keyMaterial, { name: 'AES-GCM', length: 256 }, false, ['encrypt', 'decrypt']);
}
/**
 * Generate random bytes
 */
export function randomBytes(length) {
    const bytes = new Uint8Array(length);
    crypto.getRandomValues(bytes);
    return bytes;
}
/**
 * Encrypt data with password
 */
export async function encrypt(data, password) {
    // Generate salt and IV
    const salt = randomBytes(32);
    const iv = randomBytes(12);
    // Derive key from password
    const key = await deriveKey(password, salt);
    // Encrypt data
    const ciphertext = await crypto.subtle.encrypt({ name: 'AES-GCM', iv: iv.buffer }, key, data.buffer);
    return {
        ciphertext: new Uint8Array(ciphertext),
        iv,
        salt,
        algorithm: 'AES-256-GCM',
    };
}
/**
 * Decrypt data with password
 */
export async function decrypt(encrypted, password) {
    // Derive key from password using same salt
    const key = await deriveKey(password, encrypted.salt);
    // Decrypt data
    const plaintext = await crypto.subtle.decrypt({ name: 'AES-GCM', iv: encrypted.iv.buffer }, key, encrypted.ciphertext.buffer);
    return new Uint8Array(plaintext);
}
/**
 * Serialize encrypted data for storage/transmission
 */
export function serializeEncrypted(encrypted) {
    // Format: [salt (32)] [iv (12)] [ciphertext (...)]
    const result = new Uint8Array(encrypted.salt.length + encrypted.iv.length + encrypted.ciphertext.length);
    let offset = 0;
    result.set(encrypted.salt, offset);
    offset += encrypted.salt.length;
    result.set(encrypted.iv, offset);
    offset += encrypted.iv.length;
    result.set(encrypted.ciphertext, offset);
    return result;
}
/**
 * Deserialize encrypted data
 */
export function deserializeEncrypted(data) {
    const salt = data.slice(0, 32);
    const iv = data.slice(32, 44);
    const ciphertext = data.slice(44);
    return {
        salt,
        iv,
        ciphertext,
        algorithm: 'AES-256-GCM',
    };
}
/**
 * Verify content hash
 */
export async function verifyHash(data, expectedHash) {
    const actualHash = await hashContent(data);
    return actualHash === expectedHash;
}
/**
 * Generate a secure random ID
 */
export function generateId(length = 16) {
    const bytes = randomBytes(length);
    return Buffer.from(bytes).toString('hex');
}
/**
 * Constant-time string comparison to prevent timing attacks
 */
export function secureCompare(a, b) {
    if (a.length !== b.length) {
        return false;
    }
    let result = 0;
    for (let i = 0; i < a.length; i++) {
        result |= a.charCodeAt(i) ^ b.charCodeAt(i);
    }
    return result === 0;
}
/**
 * Encrypt content and return as base64
 */
export async function encryptToBase64(data, password) {
    const encrypted = await encrypt(data, password);
    const serialized = serializeEncrypted(encrypted);
    return Buffer.from(serialized).toString('base64');
}
/**
 * Decrypt content from base64
 */
export async function decryptFromBase64(base64Data, password) {
    const serialized = Buffer.from(base64Data, 'base64');
    const encrypted = deserializeEncrypted(new Uint8Array(serialized));
    return decrypt(encrypted, password);
}
//# sourceMappingURL=crypto.js.map