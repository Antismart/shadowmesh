import { useEffect, useState } from 'react';
import { useParams, Link } from 'react-router-dom';
import { deployments as deploymentsApi } from '../api/deployments';
import type { Deployment } from '../api/types';
import StatusBadge from '../components/StatusBadge';
import DeploymentCard from '../components/DeploymentCard';
import LoadingSkeleton from '../components/LoadingSkeleton';
import EmptyState from '../components/EmptyState';

export default function ProjectDetailPage() {
  const { repoName } = useParams<{ repoName: string }>();
  const [deps, setDeps] = useState<Deployment[]>([]);
  const [loading, setLoading] = useState(true);
  const [activeTab, setActiveTab] = useState<'project' | 'deployments'>('project');

  useEffect(() => {
    deploymentsApi
      .list()
      .then((all) => {
        const filtered = all.filter(
          (d) =>
            d.name === repoName ||
            d.repo_url?.includes(repoName ?? '') ||
            d.repo_url?.split('/').pop() === repoName,
        );
        setDeps(filtered);
      })
      .catch(() => setDeps([]))
      .finally(() => setLoading(false));
  }, [repoName]);

  const latest = deps[0];
  const tabs = [
    { key: 'project' as const, label: 'Project' },
    { key: 'deployments' as const, label: 'Deployments' },
  ];

  return (
    <div>
      {/* Breadcrumb */}
      <div className="flex items-center gap-2 text-sm text-mesh-muted mb-4">
        <Link to="/" className="hover:text-mesh-text transition-colors">Projects</Link>
        <span className="text-mesh-border">/</span>
        <span className="text-mesh-text">{repoName}</span>
      </div>

      {/* Title */}
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-2xl font-semibold text-mesh-text">{repoName}</h1>
          {latest?.repo_url && (
            <a
              href={latest.repo_url}
              target="_blank"
              rel="noopener noreferrer"
              className="text-sm text-mesh-muted hover:text-mesh-text transition-colors"
            >
              {latest.repo_url}
            </a>
          )}
        </div>
        {latest && (
          <a
            href={`/ipfs/${latest.cid}/index.html`}
            target="_blank"
            rel="noopener noreferrer"
            className="btn-primary"
          >
            Visit
          </a>
        )}
      </div>

      {/* Tabs */}
      <div className="flex gap-6 border-b border-mesh-border mb-8">
        {tabs.map((tab) => (
          <button
            key={tab.key}
            onClick={() => setActiveTab(tab.key)}
            className={`pb-3 text-sm font-medium border-b-2 transition-colors ${
              activeTab === tab.key
                ? 'text-mesh-text border-mesh-text'
                : 'text-mesh-muted border-transparent hover:text-mesh-text'
            }`}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Project tab */}
      {activeTab === 'project' && (
        <>
          {latest && (
            <div className="border border-mesh-border rounded-lg p-6 mb-8">
              <div className="flex items-center justify-between">
                <div>
                  <p className="text-xs text-mesh-muted uppercase tracking-wider mb-1">Production Deployment</p>
                  <a
                    href={`/ipfs/${latest.cid}/index.html`}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-sm text-mesh-accent hover:underline"
                  >
                    /ipfs/{latest.cid.slice(0, 12)}...
                  </a>
                </div>
                <div className="text-right">
                  <StatusBadge status={latest.build_status} />
                  <p className="text-xs text-mesh-muted mt-1">{latest.created_at}</p>
                </div>
              </div>
            </div>
          )}
        </>
      )}

      {/* Deployments tab */}
      {activeTab === 'deployments' && (
        <>
          {loading ? (
            <LoadingSkeleton count={3} />
          ) : deps.length === 0 ? (
            <EmptyState
              title="No deployments found"
              description={`No deployments found for ${repoName}.`}
            />
          ) : (
            <div className="border border-mesh-border rounded-lg overflow-hidden">
              {deps.map((d) => (
                <DeploymentCard key={d.cid} deployment={d} />
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}
