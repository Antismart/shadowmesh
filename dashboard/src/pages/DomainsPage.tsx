import { useState, useEffect } from 'react';
import { names as namesApi } from '../api/names';
import { deployments as deploymentsApi } from '../api/deployments';
import { useToast } from '../context/ToastContext';
import type { NameRecord, Deployment } from '../api/types';

function extractCid(record: NameRecord): string | null {
  for (const r of record.records) {
    if (r.Content?.cid) return r.Content.cid;
  }
  return null;
}

function formatDate(unix: number): string {
  return new Date(unix * 1000).toLocaleDateString('en-US', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

export default function DomainsPage() {
  const { addToast } = useToast();

  // Registration form
  const [newName, setNewName] = useState('');
  const [selectedCid, setSelectedCid] = useState('');
  const [registering, setRegistering] = useState(false);
  const [availableDeployments, setAvailableDeployments] = useState<Deployment[]>([]);

  // Owned names
  const [ownedNames, setOwnedNames] = useState<NameRecord[]>([]);
  const [loadingNames, setLoadingNames] = useState(true);
  const [deletingName, setDeletingName] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  // Editing (reassign CID)
  const [editingName, setEditingName] = useState<string | null>(null);
  const [editCid, setEditCid] = useState('');
  const [updating, setUpdating] = useState(false);

  // Resolver
  const [searchName, setSearchName] = useState('');
  const [resolveResult, setResolveResult] = useState<{ found: boolean; record?: NameRecord } | null>(null);
  const [searching, setSearching] = useState(false);

  // Load owned names and deployments on mount
  useEffect(() => {
    loadOwnedNames();
    deploymentsApi.list().then(setAvailableDeployments).catch(() => {});
  }, []);

  const loadOwnedNames = async () => {
    setLoadingNames(true);
    try {
      const names = await namesApi.list();
      setOwnedNames(names);
    } catch {
      addToast('error', 'Failed to load domains');
    } finally {
      setLoadingNames(false);
    }
  };

  const handleRegister = async () => {
    if (!newName || !selectedCid) return;
    setRegistering(true);
    try {
      const name = newName.endsWith('.shadow') ? newName : newName;
      await namesApi.assign(name, selectedCid);
      addToast('success', `${name}.shadow registered`);
      setNewName('');
      setSelectedCid('');
      loadOwnedNames();
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : 'Registration failed';
      addToast('error', msg);
    } finally {
      setRegistering(false);
    }
  };

  const handleDelete = async (name: string) => {
    setDeletingName(name);
    try {
      await namesApi.remove(name);
      addToast('success', `${name} deleted`);
      setConfirmDelete(null);
      loadOwnedNames();
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : 'Delete failed';
      addToast('error', msg);
    } finally {
      setDeletingName(null);
    }
  };

  const handleUpdate = async (name: string) => {
    if (!editCid) return;
    setUpdating(true);
    try {
      await namesApi.assign(name, editCid);
      addToast('success', `${name} updated`);
      setEditingName(null);
      setEditCid('');
      loadOwnedNames();
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : 'Update failed';
      addToast('error', msg);
    } finally {
      setUpdating(false);
    }
  };

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
        <p className="text-sm text-mesh-muted mt-1">Register and manage .shadow domain names</p>
      </div>

      {/* Register Domain */}
      <div className="border border-mesh-border rounded-lg p-6 mb-8 max-w-xl">
        <h2 className="text-sm font-medium text-mesh-text mb-4">Register Domain</h2>
        <div className="space-y-3">
          <div>
            <label className="block text-xs text-mesh-muted mb-1">Domain Name</label>
            <div className="relative">
              <input
                type="text"
                value={newName}
                onChange={(e) => setNewName(e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, ''))}
                placeholder="myapp"
                className="input w-full pr-16"
                maxLength={63}
              />
              <span className="absolute right-3 top-1/2 -translate-y-1/2 text-xs text-mesh-muted">.shadow</span>
            </div>
          </div>
          <div>
            <label className="block text-xs text-mesh-muted mb-1">Target Deployment</label>
            <select
              value={selectedCid}
              onChange={(e) => setSelectedCid(e.target.value)}
              className="input w-full"
            >
              <option value="">Select a deployment...</option>
              {availableDeployments.map((d) => (
                <option key={d.cid} value={d.cid}>
                  {d.name} ({d.cid.slice(0, 12)}...)
                </option>
              ))}
            </select>
          </div>
          <button
            onClick={handleRegister}
            disabled={registering || !newName || !selectedCid}
            className="btn-primary w-full"
          >
            {registering ? 'Registering...' : 'Register Domain'}
          </button>
        </div>
      </div>

      {/* Your Domains */}
      <div className="border border-mesh-border rounded-lg p-6 mb-8 max-w-xl">
        <h2 className="text-sm font-medium text-mesh-text mb-4">Your Domains</h2>
        {loadingNames ? (
          <p className="text-sm text-mesh-muted">Loading...</p>
        ) : ownedNames.length === 0 ? (
          <p className="text-sm text-mesh-muted">No domains registered yet. Register one above to get started.</p>
        ) : (
          <div className="space-y-3">
            {ownedNames.map((record) => {
              const cid = extractCid(record);
              const isEditing = editingName === record.name;
              const isDeleting = deletingName === record.name;

              return (
                <div key={record.name_hash} className="border border-mesh-border rounded-md p-4">
                  <div className="flex items-center justify-between mb-2">
                    <span className="text-sm font-medium text-mesh-text">{record.name}</span>
                    <span className="text-xs text-mesh-muted">{formatDate(record.created_at)}</span>
                  </div>

                  {cid && !isEditing && (
                    <div className="flex items-center gap-2 mb-3">
                      <span className="text-xs text-mesh-muted">CID:</span>
                      <a
                        href={`/ipfs/${cid}/index.html`}
                        className="text-xs font-mono text-blue-400 hover:text-blue-300 truncate"
                        target="_blank"
                        rel="noopener noreferrer"
                      >
                        {cid.length > 20 ? `${cid.slice(0, 20)}...` : cid}
                      </a>
                      <button
                        onClick={() => { navigator.clipboard.writeText(cid); addToast('info', 'CID copied'); }}
                        className="text-xs text-mesh-muted hover:text-mesh-text"
                        title="Copy CID"
                      >
                        Copy
                      </button>
                    </div>
                  )}

                  {isEditing ? (
                    <div className="space-y-2">
                      <select
                        value={editCid}
                        onChange={(e) => setEditCid(e.target.value)}
                        className="input w-full text-sm"
                      >
                        <option value="">Select new deployment...</option>
                        {availableDeployments.map((d) => (
                          <option key={d.cid} value={d.cid}>
                            {d.name} ({d.cid.slice(0, 12)}...)
                          </option>
                        ))}
                      </select>
                      <div className="flex gap-2">
                        <button
                          onClick={() => handleUpdate(record.name)}
                          disabled={updating || !editCid}
                          className="btn-primary text-xs"
                        >
                          {updating ? 'Updating...' : 'Save'}
                        </button>
                        <button
                          onClick={() => { setEditingName(null); setEditCid(''); }}
                          className="btn-ghost text-xs"
                        >
                          Cancel
                        </button>
                      </div>
                    </div>
                  ) : (
                    <div className="flex gap-2">
                      <button
                        onClick={() => { setEditingName(record.name); setEditCid(cid || ''); }}
                        className="btn-ghost text-xs"
                      >
                        Update
                      </button>
                      {confirmDelete === record.name ? (
                        <>
                          <button
                            onClick={() => handleDelete(record.name)}
                            disabled={isDeleting}
                            className="btn-danger text-xs"
                          >
                            {isDeleting ? 'Deleting...' : 'Confirm'}
                          </button>
                          <button
                            onClick={() => setConfirmDelete(null)}
                            className="btn-ghost text-xs"
                          >
                            Cancel
                          </button>
                        </>
                      ) : (
                        <button
                          onClick={() => setConfirmDelete(record.name)}
                          className="btn-ghost text-xs text-red-400 hover:text-red-300"
                        >
                          Delete
                        </button>
                      )}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Resolve Domain */}
      <div className="border border-mesh-border rounded-lg p-6 mb-8 max-w-xl">
        <h2 className="text-sm font-medium text-mesh-text mb-4">Resolve a Domain</h2>
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

      {/* Resolve result */}
      {resolveResult && (
        <div className="border border-mesh-border rounded-lg p-6 mb-8 max-w-xl">
          <h3 className="text-sm font-medium text-mesh-text mb-3">
            {resolveResult.found ? 'Record Found' : 'Not Found'}
          </h3>
          {resolveResult.found && resolveResult.record ? (
            <div className="space-y-2">
              <div className="text-xs text-mesh-muted">
                <span className="font-medium text-mesh-text">{resolveResult.record.name}</span>
              </div>
              {extractCid(resolveResult.record) && (
                <div className="flex items-center gap-2">
                  <span className="text-xs text-mesh-muted">Points to:</span>
                  <a
                    href={`/ipfs/${extractCid(resolveResult.record)}/index.html`}
                    className="text-xs font-mono text-blue-400 hover:text-blue-300"
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    {extractCid(resolveResult.record)}
                  </a>
                </div>
              )}
              <pre className="text-xs font-mono text-mesh-muted bg-mesh-bg rounded-md p-4 overflow-x-auto mt-2">
                {JSON.stringify(resolveResult.record, null, 2)}
              </pre>
            </div>
          ) : (
            <p className="text-sm text-mesh-muted">
              This name is available. Register it above.
            </p>
          )}
        </div>
      )}

      {/* Info section */}
      <div className="border border-mesh-border rounded-lg p-6 max-w-xl">
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
