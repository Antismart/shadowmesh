import { useEffect, useState } from 'react';
import { keys as keysApi } from '../api/keys';
import { metrics as metricsApi } from '../api/metrics';
import type { ApiKeyInfo, HealthResponse } from '../api/types';
import { useAuth } from '../context/AuthContext';
import { useToast } from '../context/ToastContext';
import Modal from '../components/Modal';
import LoadingSkeleton from '../components/LoadingSkeleton';
import CopyButton from '../components/CopyButton';

export default function SettingsPage() {
  const { connected, user } = useAuth();
  const { addToast } = useToast();
  const [apiKeys, setApiKeys] = useState<ApiKeyInfo[]>([]);
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [showCreate, setShowCreate] = useState(false);
  const [newKeyName, setNewKeyName] = useState('');
  const [createdKey, setCreatedKey] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  const fetchKeys = () => {
    keysApi.list().then(setApiKeys).catch(() => {});
  };

  useEffect(() => {
    Promise.all([
      keysApi.list().catch(() => []),
      metricsApi.health().catch(() => null),
    ]).then(([k, h]) => {
      setApiKeys(k);
      setHealth(h);
      setLoading(false);
    });
  }, []);

  const handleCreate = async () => {
    if (!newKeyName.trim()) return;
    setCreating(true);
    try {
      const result = await keysApi.create(newKeyName.trim());
      setCreatedKey(result.key);
      addToast('success', 'API key created');
      fetchKeys();
      setNewKeyName('');
    } catch {
      addToast('error', 'Failed to create API key');
    } finally {
      setCreating(false);
    }
  };

  const handleRevoke = async (id: string) => {
    try {
      await keysApi.revoke(id);
      addToast('success', 'API key revoked');
      fetchKeys();
    } catch {
      addToast('error', 'Failed to revoke key');
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await keysApi.remove(id);
      addToast('success', 'API key deleted');
      fetchKeys();
    } catch {
      addToast('error', 'Failed to delete key');
    }
  };

  if (loading) return <LoadingSkeleton count={3} />;

  return (
    <div>
      <h1 className="text-2xl font-semibold text-mesh-text mb-8">Settings</h1>

      {/* Account */}
      <section className="mb-8">
        <h2 className="text-sm font-medium text-mesh-muted uppercase tracking-wider mb-3">Account</h2>
        <div className="border border-mesh-border rounded-lg p-5">
          {connected && user ? (
            <div className="flex items-center gap-3">
              <div className="w-10 h-10 rounded-full bg-mesh-surface flex items-center justify-center">
                <span className="text-lg text-mesh-text">{user.login[0].toUpperCase()}</span>
              </div>
              <div>
                <p className="text-sm font-medium text-mesh-text">{user.login}</p>
                <p className="text-xs text-mesh-muted">Connected via GitHub</p>
              </div>
            </div>
          ) : (
            <div className="flex items-center justify-between">
              <p className="text-sm text-mesh-muted">No GitHub account connected</p>
              <a href="/api/github/login" className="btn-primary text-sm">Connect GitHub</a>
            </div>
          )}
        </div>
      </section>

      {/* Gateway Info */}
      <section className="mb-8">
        <h2 className="text-sm font-medium text-mesh-muted uppercase tracking-wider mb-3">Gateway</h2>
        <div className="border border-mesh-border rounded-lg p-5">
          <div className="grid grid-cols-2 gap-y-3 text-sm">
            <span className="text-mesh-muted">Version</span>
            <span className="text-mesh-text">{health?.version ?? '-'}</span>
            <span className="text-mesh-muted">Status</span>
            <span className={health?.status === 'healthy' ? 'text-mesh-accent' : 'text-[#ee0000]'}>{health?.status ?? '-'}</span>
            <span className="text-mesh-muted">IPFS</span>
            <span className={health?.ipfs_connected ? 'text-mesh-accent' : 'text-mesh-muted'}>{health?.ipfs_connected ? 'Connected' : 'Disconnected'}</span>
          </div>
        </div>
      </section>

      {/* API Keys */}
      <section>
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-sm font-medium text-mesh-muted uppercase tracking-wider">API Keys</h2>
          <button onClick={() => { setShowCreate(true); setCreatedKey(null); }} className="btn-secondary text-sm">
            Create Key
          </button>
        </div>

        {apiKeys.length === 0 ? (
          <div className="border border-mesh-border rounded-lg p-8 text-center">
            <p className="text-sm text-mesh-muted">No API keys yet. Create one to use the API programmatically.</p>
          </div>
        ) : (
          <div className="border border-mesh-border rounded-lg overflow-hidden">
            {apiKeys.map((key) => (
              <div key={key.id} className="flex items-center justify-between px-4 py-3 border-b border-mesh-border last:border-b-0">
                <div>
                  <p className="text-sm font-medium text-mesh-text">{key.name}</p>
                  <p className="text-xs text-mesh-muted">
                    {key.prefix}... · Created {key.created_at}
                    {key.last_used && ` · Last used ${key.last_used}`}
                  </p>
                </div>
                <div className="flex gap-3">
                  <button
                    onClick={() => handleRevoke(key.id)}
                    className="text-xs text-mesh-muted hover:text-mesh-text transition-colors"
                    disabled={!key.enabled}
                  >
                    {key.enabled ? 'Revoke' : 'Revoked'}
                  </button>
                  <button
                    onClick={() => handleDelete(key.id)}
                    className="text-xs text-mesh-muted hover:text-[#ee0000] transition-colors"
                  >
                    Delete
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </section>

      {/* Create key modal */}
      <Modal open={showCreate} onClose={() => setShowCreate(false)} title="Create API Key">
        {createdKey ? (
          <div>
            <p className="text-sm text-mesh-muted mb-3">
              Save this key now. You won't be able to see it again.
            </p>
            <div className="flex items-center gap-2 bg-mesh-bg border border-mesh-border rounded-md p-3">
              <code className="text-sm font-mono text-mesh-accent flex-1 break-all">{createdKey}</code>
              <CopyButton text={createdKey} />
            </div>
            <button onClick={() => setShowCreate(false)} className="btn-primary w-full mt-4">
              Done
            </button>
          </div>
        ) : (
          <div className="space-y-4">
            <div>
              <label className="block text-sm font-medium text-mesh-text mb-2">Key Name</label>
              <input
                type="text"
                value={newKeyName}
                onChange={(e) => setNewKeyName(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handleCreate()}
                placeholder="e.g., CI/CD Pipeline"
                className="input w-full"
              />
            </div>
            <button onClick={handleCreate} disabled={!newKeyName.trim() || creating} className="btn-primary w-full">
              {creating ? 'Creating...' : 'Create Key'}
            </button>
          </div>
        )}
      </Modal>
    </div>
  );
}
