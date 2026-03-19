import { createContext, useContext, useEffect, useState, type ReactNode } from 'react';
import { github } from '../api/github';
import { setAuthToken, getAuthToken } from '../api/client';
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

export function AuthProvider({ children }: { children: ReactNode }) {
  const [connected, setConnected] = useState(false);
  const [user, setUser] = useState<GithubUser | null>(null);
  const [loading, setLoading] = useState(true);

  // On mount, check for ?token= in the URL (from OAuth redirect)
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const token = params.get('token');
    if (token) {
      setAuthToken(token);
      // Remove token from URL without reload
      params.delete('token');
      const clean = params.toString();
      const newUrl = window.location.pathname + (clean ? `?${clean}` : '');
      window.history.replaceState({}, '', newUrl);
    }
  }, []);

  const refresh = async () => {
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

  const logout = () => {
    setAuthToken(null);
    setConnected(false);
    setUser(null);
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
