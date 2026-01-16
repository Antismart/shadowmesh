/**
 * ShadowMesh SDK Types
 *
 * Comprehensive TypeScript types for the ShadowMesh SDK.
 */
// ============================================================================
// Error Types
// ============================================================================
/**
 * ShadowMesh error codes
 */
export var ErrorCode;
(function (ErrorCode) {
    // Network errors
    ErrorCode["NETWORK_ERROR"] = "NETWORK_ERROR";
    ErrorCode["TIMEOUT"] = "TIMEOUT";
    ErrorCode["CONNECTION_REFUSED"] = "CONNECTION_REFUSED";
    // Content errors
    ErrorCode["CONTENT_NOT_FOUND"] = "CONTENT_NOT_FOUND";
    ErrorCode["CONTENT_UNAVAILABLE"] = "CONTENT_UNAVAILABLE";
    ErrorCode["INVALID_CID"] = "INVALID_CID";
    ErrorCode["HASH_MISMATCH"] = "HASH_MISMATCH";
    // Encryption errors
    ErrorCode["ENCRYPTION_FAILED"] = "ENCRYPTION_FAILED";
    ErrorCode["DECRYPTION_FAILED"] = "DECRYPTION_FAILED";
    ErrorCode["INVALID_PASSWORD"] = "INVALID_PASSWORD";
    // Upload errors
    ErrorCode["UPLOAD_FAILED"] = "UPLOAD_FAILED";
    ErrorCode["FRAGMENT_FAILED"] = "FRAGMENT_FAILED";
    ErrorCode["STORAGE_FULL"] = "STORAGE_FULL";
    // API errors
    ErrorCode["UNAUTHORIZED"] = "UNAUTHORIZED";
    ErrorCode["RATE_LIMITED"] = "RATE_LIMITED";
    ErrorCode["INVALID_REQUEST"] = "INVALID_REQUEST";
    // Other
    ErrorCode["UNKNOWN_ERROR"] = "UNKNOWN_ERROR";
})(ErrorCode || (ErrorCode = {}));
//# sourceMappingURL=types.js.map