import { useState } from 'react';
import { names as namesApi } from '../api/names';
import { useToast } from '../context/ToastContext';

export default function DomainsPage() {
  const { addToast } = useToast();
  const [searchName, setSearchName] = useState('');
  const [resolveResult, setResolveResult] = useState<{ found: boolean; record?: unknown } | null>(null);
  const [searching, setSearching] = useState(false);

  const handleResolve = async () => {
    if (!searchName) return;
    const name = searchName.endsWith('.shadow') ? searchName : `${searchName}.shadow`;
    setSearching(true);
    try {
      const result = await namesApi.resolve(name);
      setResolveResult(result);
      if (!result.found) {
        addToast('info', `${name} is not registered`);
      }
    } catch {
      addToast('error', 'Failed to resolve name');
    } finally {
      setSearching(false);
    }
  };

  return (
    <div>
      <div className="mb-8">
        <h1 className="text-2xl font-semibold text-mesh-text">Domains</h1>
        <p className="text-sm text-mesh-muted mt-1">Manage your .shadow domain names</p>
      </div>

      {/* Name resolver */}
      <div className="border border-mesh-border rounded-lg p-6 mb-8 max-w-xl">
        <h2 className="text-sm font-medium text-mesh-text mb-4">Resolve a Name</h2>
        <div className="flex gap-2">
          <div className="flex-1 relative">
            <input
              type="text"
              value={searchName}
              onChange={(e) => setSearchName(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleResolve()}
              placeholder="myapp"
              className="input w-full pr-16"
            />
            <span className="absolute right-3 top-1/2 -translate-y-1/2 text-xs text-mesh-muted">.shadow</span>
          </div>
          <button onClick={handleResolve} disabled={searching || !searchName} className="btn-primary">
            {searching ? 'Resolving...' : 'Resolve'}
          </button>
        </div>
      </div>

      {/* Result */}
      {resolveResult && (
        <div className="border border-mesh-border rounded-lg p-6 max-w-xl">
          <h3 className="text-sm font-medium text-mesh-text mb-3">
            {resolveResult.found ? 'Record Found' : 'Not Found'}
          </h3>
          {resolveResult.found && resolveResult.record ? (
            <pre className="text-xs font-mono text-mesh-muted bg-mesh-bg rounded-md p-4 overflow-x-auto">
              {JSON.stringify(resolveResult.record, null, 2)}
            </pre>
          ) : (
            <p className="text-sm text-mesh-muted">
              This name is available. Register it via the CLI or API.
            </p>
          )}
        </div>
      )}

      {/* Info section */}
      <div className="mt-8 border border-mesh-border rounded-lg p-6 max-w-xl">
        <h2 className="text-sm font-medium text-mesh-text mb-3">About .shadow Names</h2>
        <ul className="space-y-2 text-sm text-mesh-muted">
          <li className="flex gap-2">
            <span className="text-mesh-muted">-</span>
            Decentralized naming via Kademlia DHT
          </li>
          <li className="flex gap-2">
            <span className="text-mesh-muted">-</span>
            No DNS dependency - works even under censorship
          </li>
          <li className="flex gap-2">
            <span className="text-mesh-muted">-</span>
            Ownership proven by Ed25519 signatures
          </li>
          <li className="flex gap-2">
            <span className="text-mesh-muted">-</span>
            Map names to content, gateways, or services
          </li>
        </ul>
      </div>
    </div>
  );
}
