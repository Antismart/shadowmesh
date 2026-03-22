import { useState, useEffect } from 'react';
import { names as namesApi } from '../api/names';
import { deployments as deploymentsApi } from '../api/deployments';
import { useToast } from '../context/ToastContext';
import CopyButton from '../components/CopyButton';
import Modal from '../components/Modal';
import EmptyState from '../components/EmptyState';
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

function findDeploymentByCid(deployments: Deployment[], cid: string): Deployment | undefined {
  return deployments.find((d) => d.cid === cid);
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

  // Editing (reassign CID) modal
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
      const name = newName.replace(/\.shadow$/, '');
      await namesApi.assign(name, selectedCid);
      addToast('success', `${name}.shadow registered successfully`);
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
      addToast('success', `${name} updated to new deployment`);
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

  const successfulDeployments = availableDeployments.filter(
    (d) => d.build_status === 'Built' || d.build_status === 'Uploaded'
  );

  return (
    <div>
      {/* Page header */}
      <div className="mb-8">
        <h1 className="text-2xl font-semibold text-mesh-text">Domains</h1>
        <p className="text-sm text-mesh-muted mt-1">Register and manage .shadow domain names for your deployments</p>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-8">
        {/* Left column: Registration form + Resolver */}
        <div className="lg:col-span-1 space-y-6">
          {/* Register Domain */}
          <div className="border border-mesh-border rounded-lg p-6">
            <div className="flex items-center gap-2 mb-4">
              <svg className="w-4 h-4 text-mesh-accent" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
              </svg>
              <h2 className="text-sm font-medium text-mesh-text">Register Domain</h2>
            </div>
            <div className="space-y-3">
              <div>
                <label className="block text-xs text-mesh-muted mb-1.5">Domain Name</label>
                <div className="relative">
                  <input
                    type="text"
                    value={newName}
                    onChange={(e) => setNewName(e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, ''))}
                    placeholder="myapp"
                    className="input w-full pr-16"
                    maxLength={63}
                    onKeyDown={(e) => e.key === 'Enter' && handleRegister()}
                  />
                  <span className="absolute right-3 top-1/2 -translate-y-1/2 text-xs text-mesh-muted font-mono">.shadow</span>
                </div>
                {newName && (
                  <p className="text-xs text-mesh-muted mt-1">
                    Will register as <span className="font-mono text-mesh-accent">{newName}.shadow</span>
                  </p>
                )}
              </div>
              <div>
                <label className="block text-xs text-mesh-muted mb-1.5">Target Deployment</label>
                <select
                  value={selectedCid}
                  onChange={(e) => setSelectedCid(e.target.value)}
                  className="input w-full"
                >
                  <option value="">Select a deployment...</option>
                  {successfulDeployments.map((d) => (
                    <option key={d.cid} value={d.cid}>
                      {d.name} ({d.cid.slice(0, 12)}...)
                    </option>
                  ))}
                </select>
                {successfulDeployments.length === 0 && (
                  <p className="text-xs text-mesh-muted mt-1">No successful deployments available. Deploy a project first.</p>
                )}
              </div>
              <button
                onClick={handleRegister}
                disabled={registering || !newName || !selectedCid}
                className="btn-primary w-full"
              >
                {registering ? (
                  <span className="flex items-center justify-center gap-2">
                    <span className="w-3.5 h-3.5 border-2 border-black/30 border-t-black rounded-full animate-spin" />
                    Registering...
                  </span>
                ) : (
                  'Register Domain'
                )}
              </button>
            </div>
          </div>

          {/* Resolve Domain */}
          <div className="border border-mesh-border rounded-lg p-6">
            <div className="flex items-center gap-2 mb-4">
              <svg className="w-4 h-4 text-mesh-muted" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="m21 21-5.197-5.197m0 0A7.5 7.5 0 1 0 5.196 5.196a7.5 7.5 0 0 0 10.607 10.607Z" />
              </svg>
              <h2 className="text-sm font-medium text-mesh-text">Resolve Domain</h2>
            </div>
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
                <span className="absolute right-3 top-1/2 -translate-y-1/2 text-xs text-mesh-muted font-mono">.shadow</span>
              </div>
              <button onClick={handleResolve} disabled={searching || !searchName} className="btn-secondary">
                {searching ? (
                  <span className="w-3.5 h-3.5 border-2 border-mesh-text/30 border-t-mesh-text rounded-full animate-spin" />
                ) : (
                  'Lookup'
                )}
              </button>
            </div>

            {/* Resolve result inline */}
            {resolveResult && (
              <div className="mt-4 pt-4 border-t border-mesh-border">
                {resolveResult.found && resolveResult.record ? (
                  <div className="space-y-2">
                    <div className="flex items-center gap-2">
                      <span className="w-2 h-2 rounded-full bg-mesh-accent flex-shrink-0" />
                      <span className="text-sm font-medium text-mesh-text">{resolveResult.record.name}</span>
                    </div>
                    {extractCid(resolveResult.record) && (
                      <div className="flex items-center gap-2">
                        <span className="text-xs text-mesh-muted">CID:</span>
                        <code className="text-xs font-mono text-mesh-accent truncate">{extractCid(resolveResult.record)}</code>
                        <CopyButton text={extractCid(resolveResult.record)!} />
                      </div>
                    )}
                    <a
                      href={`/ipfs/${extractCid(resolveResult.record)}/index.html`}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="text-xs text-mesh-accent hover:underline inline-flex items-center gap-1"
                    >
                      Visit site
                      <svg className="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                        <path strokeLinecap="round" strokeLinejoin="round" d="M13.5 6H5.25A2.25 2.25 0 003 8.25v10.5A2.25 2.25 0 005.25 21h10.5A2.25 2.25 0 0018 18.75V10.5m-10.5 6L21 3m0 0h-5.25M21 3v5.25" />
                      </svg>
                    </a>
                  </div>
                ) : (
                  <div className="flex items-center gap-2">
                    <span className="w-2 h-2 rounded-full bg-mesh-muted flex-shrink-0" />
                    <span className="text-sm text-mesh-muted">Not registered -- this name is available</span>
                  </div>
                )}
              </div>
            )}
          </div>

          {/* Info card */}
          <div className="border border-mesh-border rounded-lg p-6">
            <h2 className="text-sm font-medium text-mesh-text mb-3">About .shadow Names</h2>
            <ul className="space-y-2 text-xs text-mesh-muted">
              <li className="flex gap-2 items-start">
                <span className="text-mesh-accent mt-0.5">--</span>
                Decentralized naming via Kademlia DHT
              </li>
              <li className="flex gap-2 items-start">
                <span className="text-mesh-accent mt-0.5">--</span>
                No DNS dependency -- works even under censorship
              </li>
              <li className="flex gap-2 items-start">
                <span className="text-mesh-accent mt-0.5">--</span>
                Ownership proven by Ed25519 signatures
              </li>
              <li className="flex gap-2 items-start">
                <span className="text-mesh-accent mt-0.5">--</span>
                Map names to content, gateways, or services
              </li>
            </ul>
          </div>
        </div>

        {/* Right column: Your Domains table */}
        <div className="lg:col-span-2">
          <div className="border border-mesh-border rounded-lg">
            <div className="flex items-center justify-between px-6 py-4 border-b border-mesh-border">
              <div className="flex items-center gap-2">
                <h2 className="text-sm font-medium text-mesh-text">Your Domains</h2>
                {ownedNames.length > 0 && (
                  <span className="text-xs bg-mesh-surface px-2 py-0.5 rounded-full text-mesh-muted border border-mesh-border">
                    {ownedNames.length}
                  </span>
                )}
              </div>
              <button
                onClick={loadOwnedNames}
                className="text-xs text-mesh-muted hover:text-mesh-text transition-colors flex items-center gap-1"
                title="Refresh"
              >
                <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182" />
                </svg>
                Refresh
              </button>
            </div>

            {loadingNames ? (
              <div className="p-6">
                <div className="space-y-3">
                  {[1, 2, 3].map((i) => (
                    <div key={i} className="h-16 bg-mesh-surface rounded-md animate-pulse" />
                  ))}
                </div>
              </div>
            ) : ownedNames.length === 0 ? (
              <div className="p-6">
                <EmptyState
                  title="No domains yet"
                  description="Register your first .shadow domain using the form on the left. Point it at any of your deployments."
                />
              </div>
            ) : (
              <div className="divide-y divide-mesh-border">
                {ownedNames.map((record) => {
                  const cid = extractCid(record);
                  const deployment = cid ? findDeploymentByCid(availableDeployments, cid) : undefined;
                  const isDeleting = deletingName === record.name;

                  return (
                    <div key={record.name_hash} className="px-6 py-4 hover:bg-mesh-surface/50 transition-colors">
                      <div className="flex items-start justify-between gap-4">
                        {/* Domain info */}
                        <div className="flex-1 min-w-0">
                          <div className="flex items-center gap-2 mb-1">
                            <span className="w-2 h-2 rounded-full bg-mesh-accent flex-shrink-0" />
                            <span className="text-sm font-medium text-mesh-text font-mono">{record.name}</span>
                          </div>
                          {cid && (
                            <div className="ml-4 space-y-1">
                              <div className="flex items-center gap-2">
                                <span className="text-xs text-mesh-muted">CID:</span>
                                <code className="text-xs font-mono text-mesh-muted truncate max-w-[280px]">{cid}</code>
                                <CopyButton text={cid} />
                              </div>
                              {deployment && (
                                <div className="flex items-center gap-2">
                                  <span className="text-xs text-mesh-muted">Project:</span>
                                  <a
                                    href={`/deployments/${cid}`}
                                    className="text-xs text-mesh-accent hover:underline"
                                  >
                                    {deployment.name}
                                  </a>
                                </div>
                              )}
                              <div className="flex items-center gap-3">
                                <a
                                  href={`/ipfs/${cid}/index.html`}
                                  target="_blank"
                                  rel="noopener noreferrer"
                                  className="text-xs text-mesh-accent hover:underline inline-flex items-center gap-1"
                                >
                                  Visit
                                  <svg className="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                                    <path strokeLinecap="round" strokeLinejoin="round" d="M13.5 6H5.25A2.25 2.25 0 003 8.25v10.5A2.25 2.25 0 005.25 21h10.5A2.25 2.25 0 0018 18.75V10.5m-10.5 6L21 3m0 0h-5.25M21 3v5.25" />
                                  </svg>
                                </a>
                                <span className="text-xs text-mesh-muted">{formatDate(record.created_at)}</span>
                              </div>
                            </div>
                          )}
                        </div>

                        {/* Actions */}
                        <div className="flex items-center gap-1 flex-shrink-0">
                          <button
                            onClick={() => { setEditingName(record.name); setEditCid(cid || ''); }}
                            className="text-xs text-mesh-muted hover:text-mesh-text px-2.5 py-1.5 rounded-md hover:bg-mesh-surface transition-colors"
                            title="Reassign to different deployment"
                          >
                            Reassign
                          </button>
                          {confirmDelete === record.name ? (
                            <div className="flex items-center gap-1">
                              <button
                                onClick={() => handleDelete(record.name)}
                                disabled={isDeleting}
                                className="text-xs text-red-400 hover:text-red-300 px-2.5 py-1.5 rounded-md hover:bg-red-500/10 transition-colors"
                              >
                                {isDeleting ? (
                                  <span className="flex items-center gap-1">
                                    <span className="w-3 h-3 border-2 border-red-400/30 border-t-red-400 rounded-full animate-spin" />
                                    Deleting
                                  </span>
                                ) : (
                                  'Confirm'
                                )}
                              </button>
                              <button
                                onClick={() => setConfirmDelete(null)}
                                className="text-xs text-mesh-muted hover:text-mesh-text px-2 py-1.5 rounded-md hover:bg-mesh-surface transition-colors"
                              >
                                Cancel
                              </button>
                            </div>
                          ) : (
                            <button
                              onClick={() => setConfirmDelete(record.name)}
                              className="text-xs text-mesh-muted hover:text-red-400 px-2.5 py-1.5 rounded-md hover:bg-mesh-surface transition-colors"
                              title="Delete domain"
                            >
                              <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                                <path strokeLinecap="round" strokeLinejoin="round" d="m14.74 9-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 0 1-2.244 2.077H8.084a2.25 2.25 0 0 1-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 0 0-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 0 1 3.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 0 0-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 0 0-7.5 0" />
                              </svg>
                            </button>
                          )}
                        </div>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Edit / Reassign Modal */}
      <Modal
        open={editingName !== null}
        onClose={() => { setEditingName(null); setEditCid(''); }}
        title="Reassign Domain"
      >
        {editingName && (
          <div className="space-y-4">
            <p className="text-sm text-mesh-muted">
              Update <span className="font-mono text-mesh-text">{editingName}</span> to point to a different deployment.
            </p>
            <div>
              <label className="block text-xs text-mesh-muted mb-1.5">New Target Deployment</label>
              <select
                value={editCid}
                onChange={(e) => setEditCid(e.target.value)}
                className="input w-full"
              >
                <option value="">Select a deployment...</option>
                {successfulDeployments.map((d) => (
                  <option key={d.cid} value={d.cid}>
                    {d.name} ({d.cid.slice(0, 12)}...)
                  </option>
                ))}
              </select>
            </div>
            <div className="flex justify-end gap-3">
              <button
                onClick={() => { setEditingName(null); setEditCid(''); }}
                className="btn-secondary"
                disabled={updating}
              >
                Cancel
              </button>
              <button
                onClick={() => handleUpdate(editingName)}
                disabled={updating || !editCid}
                className="btn-primary"
              >
                {updating ? (
                  <span className="flex items-center gap-2">
                    <span className="w-3.5 h-3.5 border-2 border-black/30 border-t-black rounded-full animate-spin" />
                    Updating...
                  </span>
                ) : (
                  'Update Domain'
                )}
              </button>
            </div>
          </div>
        )}
      </Modal>
    </div>
  );
}
