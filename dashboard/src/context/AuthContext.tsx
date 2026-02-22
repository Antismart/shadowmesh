import { createContext, useContext, useEffect, useState, type ReactNode } from 'react';
import { github } from '../api/github';
import type { GithubUser } from '../api/types';

interface AuthState {
  connected: boolean;
  user: GithubUser | null;
  loading: boolean;
  refresh: () => Promise<void>;
}

const AuthContext = createContext<AuthState>({
  connected: false,
  user: null,
  loading: true,
  refresh: async () => {},
});

export function AuthProvider({ children }: { children: ReactNode }) {
  const [connected, setConnected] = useState(false);
  const [user, setUser] = useState<GithubUser | null>(null);
  const [loading, setLoading] = useState(true);

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

  useEffect(() => {
    refresh();
  }, []);

  return (
    <AuthContext.Provider value={{ connected, user, loading, refresh }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  return useContext(AuthContext);
}
