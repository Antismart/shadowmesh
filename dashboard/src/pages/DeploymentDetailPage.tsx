import { useEffect, useState } from 'react';
import { useParams, Link, useNavigate } from 'react-router-dom';
import { deployments as deploymentsApi } from '../api/deployments';
import type { Deployment } from '../api/types';
import StatusBadge from '../components/StatusBadge';
import CopyButton from '../components/CopyButton';
import BuildLogViewer from '../components/BuildLogViewer';
import ConfirmDialog from '../components/ConfirmDialog';
import LoadingSkeleton from '../components/LoadingSkeleton';
import { useToast } from '../context/ToastContext';

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export default function DeploymentDetailPage() {
  const { cid } = useParams<{ cid: string }>();
  const navigate = useNavigate();
  const { addToast } = useToast();
  const [deployment, setDeployment] = useState<Deployment | null>(null);
  const [logs, setLogs] = useState('');
  const [loading, setLoading] = useState(true);
  const [showDelete, setShowDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [redeploying, setRedeploying] = useState(false);

  useEffect(() => {
    if (!cid) return;
    Promise.all([
      deploymentsApi.list().then((all) => all.find((d) => d.cid === cid) ?? null),
      deploymentsApi.logs(cid).catch(() => ({ logs: '' })),
    ]).then(([dep, logRes]) => {
      setDeployment(dep);
      setLogs(logRes.logs ?? '');
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
      await deploymentsApi.redeploy(cid);
      addToast('success', 'Redeployment started');
    } catch {
      addToast('error', 'Failed to redeploy');
    } finally {
      setRedeploying(false);
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

  const previewUrl = `/ipfs/${deployment.cid}/index.html`;

  return (
    <div>
      {/* Breadcrumb */}
      <div className="flex items-center gap-2 text-sm text-mesh-muted mb-6">
        <Link to="/" className="hover:text-mesh-text transition-colors">Projects</Link>
        <span className="text-mesh-border">/</span>
        <span className="text-mesh-text truncate">{deployment.name}</span>
      </div>

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
          {deployment.source === 'github' && (
            <button onClick={handleRedeploy} disabled={redeploying} className="btn-secondary">
              {redeploying ? 'Redeploying...' : 'Redeploy'}
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

      {/* Build logs */}
      <BuildLogViewer logs={logs} status={deployment.build_status} />

      <ConfirmDialog
        open={showDelete}
        onClose={() => setShowDelete(false)}
        onConfirm={handleDelete}
        title="Delete Deployment"
        message={`Are you sure you want to delete "${deployment.name}"? This action cannot be undone.`}
        loading={deleting}
      />
    </div>
  );
}
