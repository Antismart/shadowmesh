import { useEffect, useState } from 'react';
import { useParams, Link, useNavigate } from 'react-router-dom';
import { deployments as deploymentsApi } from '../api/deployments';
import { names as namesApi } from '../api/names';
import type { Deployment } from '../api/types';
import StatusBadge from '../components/StatusBadge';
import CopyButton from '../components/CopyButton';
import BuildLogViewer from '../components/BuildLogViewer';
import ConfirmDialog from '../components/ConfirmDialog';
import LoadingSkeleton from '../components/LoadingSkeleton';
import MetricCard from '../components/MetricCard';
import Modal from '../components/Modal';
import { useToast } from '../context/ToastContext';

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatBandwidth(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

export default function DeploymentDetailPage() {
  const { cid } = useParams<{ cid: string }>();
  const navigate = useNavigate();
  const { addToast } = useToast();
  const [deployment, setDeployment] = useState<Deployment | null>(null);
  const [logs, setLogs] = useState('');
  const [loading, setLoading] = useState(true);
  const [showDelete, setShowDelete] = useState(false);
  const [showRedeploy, setShowRedeploy] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [redeploying, setRedeploying] = useState(false);

  // Analytics state
  const [analytics, setAnalytics] = useState<{ requests: number; bytes_served: number } | null>(null);

  // Quick-assign domain state
  const [showAssignDomain, setShowAssignDomain] = useState(false);
  const [domainName, setDomainName] = useState('');
  const [assigningDomain, setAssigningDomain] = useState(false);

  useEffect(() => {
    if (!cid) return;
    Promise.all([
      deploymentsApi.list().then((all) => all.find((d) => d.cid === cid) ?? null),
      deploymentsApi.logs(cid).catch(() => ({ logs: '' })),
      deploymentsApi.analytics(cid).catch(() => null),
    ]).then(([dep, logRes, analyticsRes]) => {
      setDeployment(dep);
      setLogs(logRes.logs ?? '');
      setAnalytics(analyticsRes);
      setLoading(false);
    });
  }, [cid]);

  const handleDelete = async () => {
    if (!cid) return;
    setDeleting(true);
    try {
      await deploymentsApi.remove(cid);
      addToast('success', 'Deployment deleted');
      navigate('/');
    } catch {
      addToast('error', 'Failed to delete deployment');
    } finally {
      setDeleting(false);
      setShowDelete(false);
    }
  };

  const handleRedeploy = async () => {
    if (!cid) return;
    setRedeploying(true);
    try {
      const result = await deploymentsApi.redeploy(cid);
      addToast('success', 'Redeployment successful');
      setShowRedeploy(false);
      navigate(`/deployments/${result.cid}`);
    } catch {
      addToast('error', 'Failed to redeploy');
    } finally {
      setRedeploying(false);
    }
  };

  const handleAssignDomain = async () => {
    if (!cid || !domainName) return;
    setAssigningDomain(true);
    try {
      const name = domainName.toLowerCase().replace(/[^a-z0-9-]/g, '').replace(/\.shadow$/, '');
      await namesApi.assign(name, cid);
      addToast('success', `${name}.shadow assigned to this deployment`);
      setShowAssignDomain(false);
      setDomainName('');
      // Refresh deployment to pick up the new domain
      const all = await deploymentsApi.list();
      const updated = all.find((d) => d.cid === cid) ?? null;
      if (updated) setDeployment(updated);
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to assign domain';
      addToast('error', msg);
    } finally {
      setAssigningDomain(false);
    }
  };

  if (loading) return <LoadingSkeleton count={3} />;
  if (!deployment) {
    return (
      <div className="border border-mesh-border rounded-lg p-12 text-center">
        <h2 className="text-base font-medium text-mesh-text mb-2">Deployment not found</h2>
        <Link to="/" className="text-sm text-mesh-accent hover:underline">Back to projects</Link>
      </div>
    );
  }

  const previewUrl = `/${deployment.cid}`;

  return (
    <div>
      {/* Breadcrumb */}
      <div className="flex items-center gap-2 text-sm text-mesh-muted mb-6">
        <Link to="/" className="hover:text-mesh-text transition-colors">Projects</Link>
        <span className="text-mesh-border">/</span>
        <span className="text-mesh-text truncate">{deployment.name}</span>
      </div>

      {/* Status Banner */}
      {(deployment.build_status === 'Built' || deployment.build_status === 'Uploaded') && (
        <div className="mb-6 border border-mesh-accent/30 rounded-lg bg-mesh-accent/5 p-3 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <svg className="w-4 h-4 text-mesh-accent" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <span className="text-sm text-mesh-accent">Deployment is live</span>
            <a href={previewUrl} target="_blank" rel="noopener noreferrer" className="text-xs text-mesh-muted hover:text-mesh-accent font-mono ml-2">
              {previewUrl}
            </a>
          </div>
          <a href={previewUrl} target="_blank" rel="noopener noreferrer" className="text-xs px-3 py-1 bg-mesh-accent text-black rounded font-medium hover:bg-mesh-accent/90 transition-colors">
            Visit
          </a>
        </div>
      )}
      {deployment.build_status === 'Failed' && (
        <div className="mb-6 border border-[#ee0000]/30 rounded-lg bg-[#ee0000]/5 p-3 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <svg className="w-4 h-4 text-[#ee0000]" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M9.75 9.75l4.5 4.5m0-4.5l-4.5 4.5M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <span className="text-sm text-[#ee0000]">Build failed — check logs below</span>
          </div>
          {deployment.source === 'github' && (
            <button onClick={() => setShowRedeploy(true)} className="text-xs px-3 py-1 border border-mesh-border rounded text-mesh-text hover:bg-mesh-surface transition-colors">
              Retry
            </button>
          )}
        </div>
      )}

      {/* Header */}
      <div className="flex items-start justify-between mb-8">
        <div>
          <div className="flex items-center gap-3 mb-2">
            <h1 className="text-2xl font-semibold text-mesh-text">{deployment.name}</h1>
            <StatusBadge status={deployment.build_status} />
          </div>
          <p className="text-sm text-mesh-muted">{deployment.created_at}</p>
        </div>
        <div className="flex gap-2">
          <a href={previewUrl} target="_blank" rel="noopener noreferrer" className="btn-primary">
            Visit
          </a>
          <button
            onClick={() => setShowAssignDomain(true)}
            className="btn-secondary inline-flex items-center gap-1.5"
          >
            <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 21a9.004 9.004 0 0 0 8.716-6.747M12 21a9.004 9.004 0 0 1-8.716-6.747M12 21c2.485 0 4.5-4.03 4.5-9S14.485 3 12 3m0 18c-2.485 0-4.5-4.03-4.5-9S9.515 3 12 3m0 0a8.997 8.997 0 0 1 7.843 4.582M12 3a8.997 8.997 0 0 0-7.843 4.582m15.686 0A11.953 11.953 0 0 1 12 10.5c-2.998 0-5.74-1.1-7.843-2.918m15.686 0A8.959 8.959 0 0 1 21 12c0 .778-.099 1.533-.284 2.253m0 0A17.919 17.919 0 0 1 12 16.5c-3.162 0-6.133-.815-8.716-2.247m0 0A9.015 9.015 0 0 1 3 12c0-1.605.42-3.113 1.157-4.418" />
            </svg>
            {deployment.domain ? 'Change Domain' : 'Assign Domain'}
          </button>
          {deployment.source === 'github' && (
            <button onClick={() => setShowRedeploy(true)} disabled={redeploying} className="btn-secondary">
              Redeploy
            </button>
          )}
          <button onClick={() => setShowDelete(true)} className="btn-danger">
            Delete
          </button>
        </div>
      </div>

      {/* Info grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4 mb-8">
        <div className="border border-mesh-border rounded-lg p-4 space-y-3">
          <div>
            <p className="text-xs text-mesh-muted mb-1">CID</p>
            <div className="flex items-center gap-2">
              <code className="text-sm font-mono text-mesh-text truncate">{deployment.cid}</code>
              <CopyButton text={deployment.cid} />
            </div>
          </div>
          <div>
            <p className="text-xs text-mesh-muted mb-1">Preview URL</p>
            <div className="flex items-center gap-2">
              <a href={previewUrl} target="_blank" rel="noopener noreferrer" className="text-sm text-mesh-accent hover:underline truncate">
                {previewUrl}
              </a>
              <CopyButton text={`${window.location.origin}${previewUrl}`} />
            </div>
          </div>
          <div>
            <p className="text-xs text-mesh-muted mb-1">Domain</p>
            {deployment.domain ? (
              <div className="flex items-center gap-2">
                <span className="text-sm font-mono text-mesh-accent">{deployment.domain}</span>
                <CopyButton text={deployment.domain} />
                <Link to="/domains" className="text-xs text-mesh-muted hover:text-mesh-text">Manage</Link>
              </div>
            ) : (
              <button
                onClick={() => setShowAssignDomain(true)}
                className="text-sm text-mesh-accent hover:underline inline-flex items-center gap-1"
              >
                <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
                </svg>
                Assign a .shadow domain
              </button>
            )}
          </div>
        </div>
        <div className="border border-mesh-border rounded-lg p-4 space-y-3">
          <div className="flex justify-between">
            <span className="text-xs text-mesh-muted">Source</span>
            <span className="text-sm text-mesh-text capitalize">{deployment.source}</span>
          </div>
          {deployment.branch && (
            <div className="flex justify-between">
              <span className="text-xs text-mesh-muted">Branch</span>
              <span className="text-sm text-mesh-text font-mono">{deployment.branch}</span>
            </div>
          )}
          <div className="flex justify-between">
            <span className="text-xs text-mesh-muted">Size</span>
            <span className="text-sm text-mesh-text">{formatSize(deployment.size)}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-xs text-mesh-muted">Files</span>
            <span className="text-sm text-mesh-text">{deployment.file_count}</span>
          </div>
        </div>
      </div>

      {/* Analytics */}
      {analytics && (
        <div className="mb-8">
          <h2 className="text-sm font-medium text-mesh-muted mb-3">Analytics</h2>
          <div className="grid grid-cols-2 gap-4">
            <MetricCard
              label="Total Requests"
              value={analytics.requests.toLocaleString()}
            />
            <MetricCard
              label="Bandwidth Served"
              value={formatBandwidth(analytics.bytes_served)}
            />
          </div>
        </div>
      )}

      {/* Build logs */}
      <BuildLogViewer logs={logs} status={deployment.build_status} />

      <ConfirmDialog
        open={showDelete}
        onClose={() => setShowDelete(false)}
        onConfirm={handleDelete}
        title="Delete Deployment"
        message={`Are you sure you want to delete "${deployment.name}"? This action cannot be undone.`}
        loading={deleting}
        loadingLabel="Deleting..."
      />

      <ConfirmDialog
        open={showRedeploy}
        onClose={() => setShowRedeploy(false)}
        onConfirm={handleRedeploy}
        title="Redeploy Project"
        message={`This will rebuild and redeploy from ${deployment.repo_url || 'the original repository'} (branch: ${deployment.branch || 'main'}). The current deployment will be replaced.`}
        confirmLabel="Redeploy"
        loadingLabel="Redeploying..."
        confirmVariant="primary"
        loading={redeploying}
      />

      {/* Quick Assign Domain Modal */}
      <Modal
        open={showAssignDomain}
        onClose={() => { setShowAssignDomain(false); setDomainName(''); }}
        title="Assign .shadow Domain"
      >
        <div className="space-y-4">
          <p className="text-sm text-mesh-muted">
            Give this deployment a memorable .shadow domain name. It will be accessible via the ShadowMesh network.
          </p>

          <div className="border border-mesh-border rounded-md p-3 bg-mesh-bg">
            <div className="flex items-center gap-2 text-xs text-mesh-muted mb-1">
              <span>Deployment:</span>
              <span className="font-medium text-mesh-text">{deployment.name}</span>
            </div>
            <div className="flex items-center gap-2 text-xs text-mesh-muted">
              <span>CID:</span>
              <code className="font-mono text-mesh-muted">{deployment.cid.slice(0, 24)}...</code>
            </div>
          </div>

          <div>
            <label className="block text-xs text-mesh-muted mb-1.5">Domain Name</label>
            <div className="relative">
              <input
                type="text"
                value={domainName}
                onChange={(e) => setDomainName(e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, ''))}
                placeholder={deployment.name.toLowerCase().replace(/[^a-z0-9-]/g, '-').replace(/-+/g, '-')}
                className="input w-full pr-16"
                maxLength={63}
                onKeyDown={(e) => e.key === 'Enter' && handleAssignDomain()}
                autoFocus
              />
              <span className="absolute right-3 top-1/2 -translate-y-1/2 text-xs text-mesh-muted font-mono">.shadow</span>
            </div>
            {domainName && (
              <p className="text-xs text-mesh-muted mt-1.5">
                Your site will be available at <span className="font-mono text-mesh-accent">{domainName}.shadow</span>
              </p>
            )}
          </div>

          <div className="flex justify-end gap-3 pt-2">
            <button
              onClick={() => { setShowAssignDomain(false); setDomainName(''); }}
              className="btn-secondary"
              disabled={assigningDomain}
            >
              Cancel
            </button>
            <button
              onClick={handleAssignDomain}
              disabled={assigningDomain || !domainName}
              className="btn-primary"
            >
              {assigningDomain ? (
                <span className="flex items-center gap-2">
                  <span className="w-3.5 h-3.5 border-2 border-black/30 border-t-black rounded-full animate-spin" />
                  Assigning...
                </span>
              ) : (
                'Assign Domain'
              )}
            </button>
          </div>

          <div className="pt-2 border-t border-mesh-border">
            <Link
              to="/domains"
              className="text-xs text-mesh-muted hover:text-mesh-text transition-colors inline-flex items-center gap-1"
              onClick={() => setShowAssignDomain(false)}
            >
              Manage all domains
              <svg className="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M13.5 4.5 21 12m0 0-7.5 7.5M21 12H3" />
              </svg>
            </Link>
          </div>
        </div>
      </Modal>
    </div>
  );
}
