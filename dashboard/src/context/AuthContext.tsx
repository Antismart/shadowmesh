import { createContext, useContext, useEffect, useState, useCallback, type ReactNode } from 'react';
import { github } from '../api/github';
import { setAuthToken, getAuthToken, isTokenExpired, onAuthExpired, apiPost } from '../api/client';
import type { GithubUser } from '../api/types';

interface AuthState {
  connected: boolean;
  user: GithubUser | null;
  loading: boolean;
  refresh: () => Promise<void>;
  logout: () => void;
}

const AuthContext = createContext<AuthState>({
  connected: false,
  user: null,
  loading: true,
  refresh: async () => {},
  logout: () => {},
});

/** Interval (ms) between proactive JWT expiry checks. */
const TOKEN_CHECK_INTERVAL_MS = 30_000;

export function AuthProvider({ children }: { children: ReactNode }) {
  const [connected, setConnected] = useState(false);
  const [user, setUser] = useState<GithubUser | null>(null);
  const [loading, setLoading] = useState(true);

  const logout = useCallback(() => {
    setAuthToken(null);
    setConnected(false);
    setUser(null);
  }, []);

  // On mount, check for ?code= (new auth-code flow) or legacy ?token= in
  // the URL from the OAuth redirect.  The auth-code flow exchanges the
  // short-lived code for a JWT via a POST, keeping the real token out of
  // browser history, Referer headers, and server logs.
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);

    const exchangeCode = async (code: string) => {
      // Clean the URL immediately so the code doesn't linger in the
      // address bar or get bookmarked.
      params.delete('code');
      const clean = params.toString();
      const newUrl = window.location.pathname + (clean ? `?${clean}` : '');
      window.history.replaceState({}, '', newUrl);

      try {
        const res = await apiPost<{ success: boolean; token?: string }>(
          '/api/github/exchange',
          { code },
        );
        if (res.success && res.token) {
          setAuthToken(res.token);
        }
      } catch (err) {
        console.error('[shadowmesh] Auth code exchange failed', err);
      }
    };

    const code = params.get('code');
    if (code) {
      exchangeCode(code);
      return;
    }

    // Legacy path: if the gateway still redirects with ?token= (e.g.
    // older gateway version), accept it directly.
    const token = params.get('token');
    if (token) {
      setAuthToken(token);
      params.delete('token');
      const clean = params.toString();
      const newUrl = window.location.pathname + (clean ? `?${clean}` : '');
      window.history.replaceState({}, '', newUrl);
    }
  }, []);

  // Subscribe to auth-expired events (fired on 401 or detected expiry in
  // the API client layer).  This ensures the UI reacts regardless of which
  // API call triggered the expiry.
  useEffect(() => {
    const unsubscribe = onAuthExpired(() => {
      setConnected(false);
      setUser(null);
    });
    return unsubscribe;
  }, []);

  // Periodically check whether the JWT has expired so we can log the user
  // out proactively rather than waiting for a failing API call.
  useEffect(() => {
    const id = setInterval(() => {
      if (getAuthToken() && isTokenExpired()) {
        logout();
      }
    }, TOKEN_CHECK_INTERVAL_MS);
    return () => clearInterval(id);
  }, [logout]);

  const refresh = async () => {
    // If the token is already expired, skip the network call and log out
    // immediately.
    if (getAuthToken() && isTokenExpired()) {
      logout();
      setLoading(false);
      return;
    }

    try {
      const status = await github.status();
      setConnected(status.connected);
      setUser(status.user ?? null);
    } catch {
      setConnected(false);
      setUser(null);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  return (
    <AuthContext.Provider value={{ connected, user, loading, refresh, logout }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  return useContext(AuthContext);
}
