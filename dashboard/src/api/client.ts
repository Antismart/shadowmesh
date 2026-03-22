// ── Gateway discovery & multi-gateway fetch ─────────────────────────────
//
// When the dashboard is served from the same origin as a gateway (dev proxy
// or co-located deploy), relative paths work automatically.  When the
// dashboard is deployed as static content on the network, it needs to
// discover a working gateway and route all API calls through it.

const DEFAULT_GATEWAYS: string[] = [
  '', // same-origin (relative paths) — fastest when served from a gateway
  'https://gateway.shadowmesh.network',
  'https://gateway2.shadowmesh.network',
  'http://localhost:8081',
];

/** Currently selected gateway base URL (empty string = same-origin). */
let activeGateway: string | null = null;

/** Whether initial gateway discovery has completed. */
let discoveryDone = false;

/**
 * Probe gateways until we find one that responds to /health.
 * Called lazily on the first API request.
 */
async function discoverGateway(): Promise<void> {
  if (discoveryDone) return;

  for (const gw of DEFAULT_GATEWAYS) {
    try {
      const res = await fetch(`${gw}/health`, {
        signal: AbortSignal.timeout(4000),
      });
      if (res.ok) {
        activeGateway = gw;
        discoveryDone = true;
        if (gw) console.log(`[shadowmesh] Using gateway: ${gw}`);
        return;
      }
    } catch {
      // try next
    }
  }

  // Fallback: assume same-origin and let individual requests fail visibly.
  activeGateway = '';
  discoveryDone = true;
}

/** Return the base URL to prepend to API paths. */
async function gateway(): Promise<string> {
  if (!discoveryDone) await discoverGateway();
  return activeGateway ?? '';
}

/**
 * Build a full URL for the given path, prepending the active gateway when
 * the dashboard is running outside of a gateway origin.
 */
export async function gatewayUrl(path: string): Promise<string> {
  const base = await gateway();
  return `${base}${path}`;
}

// ── JWT token management ────────────────────────────────────────────────

const TOKEN_KEY = 'shadowmesh_auth_token';

/** Listeners notified when the token is cleared due to expiry or a 401. */
type AuthExpiredListener = () => void;
const authExpiredListeners: AuthExpiredListener[] = [];

/** Register a callback invoked when the auth token expires or is rejected. */
export function onAuthExpired(listener: AuthExpiredListener): () => void {
  authExpiredListeners.push(listener);
  return () => {
    const idx = authExpiredListeners.indexOf(listener);
    if (idx >= 0) authExpiredListeners.splice(idx, 1);
  };
}

/** Clear the token and notify all listeners (logout on expiry / 401). */
function handleAuthExpired(): void {
  localStorage.removeItem(TOKEN_KEY);
  for (const listener of authExpiredListeners) {
    try { listener(); } catch { /* swallow */ }
  }
}

/** Store (or clear) the JWT auth token. */
export function setAuthToken(token: string | null): void {
  if (token) {
    localStorage.setItem(TOKEN_KEY, token);
  } else {
    localStorage.removeItem(TOKEN_KEY);
  }
}

/** Retrieve the stored JWT auth token. */
export function getAuthToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}

/**
 * Parse the payload of a JWT without verifying the signature.
 * Returns null if the token is malformed.
 */
export function parseJwtPayload(token: string): Record<string, unknown> | null {
  try {
    const parts = token.split('.');
    if (parts.length !== 3) return null;
    // Base64url → base64 → decode
    const b64 = parts[1].replace(/-/g, '+').replace(/_/g, '/');
    const json = atob(b64);
    return JSON.parse(json);
  } catch {
    return null;
  }
}

/**
 * Return true if the stored JWT has expired (or will expire within the
 * given grace period in seconds).  Returns false when there is no token.
 */
export function isTokenExpired(gracePeriodSeconds = 60): boolean {
  const token = getAuthToken();
  if (!token) return false;
  const payload = parseJwtPayload(token);
  if (!payload || typeof payload.exp !== 'number') return false;
  const nowSeconds = Math.floor(Date.now() / 1000);
  return payload.exp - gracePeriodSeconds <= nowSeconds;
}

/** Build the auth headers object (empty if no token). */
function authHeaders(): Record<string, string> {
  const token = getAuthToken();
  if (!token) return {};
  // Proactively detect expired tokens before sending the request
  if (isTokenExpired()) {
    handleAuthExpired();
    return {};
  }
  return { Authorization: `Bearer ${token}` };
}

// ── Error type ──────────────────────────────────────────────────────────

export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

// ── Fetch helpers ───────────────────────────────────────────────────────

export async function apiFetch<T>(
  path: string,
  options: RequestInit = {},
): Promise<T> {
  const url = await gatewayUrl(path);

  const res = await fetch(url, {
    ...options,
    headers: {
      ...authHeaders(),
      ...options.headers,
    },
  });

  if (!res.ok) {
    // If same-origin failed, try other gateways once
    if (activeGateway === '' && DEFAULT_GATEWAYS.length > 1) {
      for (const gw of DEFAULT_GATEWAYS.slice(1)) {
        try {
          const retry = await fetch(`${gw}${path}`, {
            ...options,
            headers: { ...authHeaders(), ...options.headers },
            signal: AbortSignal.timeout(8000),
          });
          if (retry.ok) {
            activeGateway = gw;
            console.log(`[shadowmesh] Switched gateway to: ${gw}`);
            return retry.json();
          }
        } catch {
          // try next
        }
      }
    }

    // On 401 Unauthorized, clear token and notify listeners so the UI
    // can redirect to login.
    if (res.status === 401) {
      handleAuthExpired();
    }

    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new ApiError(
      res.status,
      body.code ?? 'UNKNOWN',
      body.error ?? res.statusText,
    );
  }

  return res.json();
}

export function apiPost<T>(path: string, body: unknown): Promise<T> {
  return apiFetch<T>(path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
}

export function apiDelete<T>(path: string): Promise<T> {
  return apiFetch<T>(path, { method: 'DELETE' });
}

export function apiUpload<T>(path: string, formData: FormData): Promise<T> {
  return apiFetch<T>(path, {
    method: 'POST',
    body: formData,
  });
}
