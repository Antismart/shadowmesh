import { useEffect, useState } from 'react';
import { useNavigate, Link } from 'react-router-dom';
import { deployments as deploymentsApi } from '../api/deployments';
import type { Deployment } from '../api/types';
import StatusBadge from '../components/StatusBadge';
import LoadingSkeleton from '../components/LoadingSkeleton';
import EmptyState from '../components/EmptyState';

export default function OverviewPage() {
  const navigate = useNavigate();
  const [deploymentsList, setDeployments] = useState<Deployment[]>([]);
  const [loading, setLoading] = useState(true);
  const [search, setSearch] = useState('');

  useEffect(() => {
    deploymentsApi
      .list()
      .then(setDeployments)
      .catch(() => setDeployments([]))
      .finally(() => setLoading(false));
  }, []);

  // Group deployments by project
  const projects = new Map<string, Deployment[]>();
  for (const d of deploymentsList) {
    const key = d.repo_url?.split('/').pop() ?? d.name;
    const list = projects.get(key) ?? [];
    list.push(d);
    projects.set(key, list);
  }

  const filteredProjects = search
    ? Array.from(projects.entries()).filter(([key]) =>
        key.toLowerCase().includes(search.toLowerCase()),
      )
    : Array.from(projects.entries());

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-semibold text-mesh-text">Projects</h1>
        <button onClick={() => navigate('/new')} className="btn-primary">
          Add New...
        </button>
      </div>

      {/* Search */}
      <div className="mb-6">
        <input
          type="text"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Search projects..."
          className="input w-full max-w-sm"
        />
      </div>

      {loading ? (
        <LoadingSkeleton count={4} />
      ) : projects.size === 0 ? (
        <EmptyState
          title="No projects yet"
          description="Deploy your first project from GitHub or upload a ZIP file to get started."
          action={{ label: 'Import Project', onClick: () => navigate('/new') }}
        />
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {filteredProjects.map(([key, deps]) => {
            const latest = deps[0];
            return (
              <Link
                key={key}
                to={`/projects/${key}`}
                className="border border-mesh-border rounded-lg p-4 hover:border-mesh-border-hover transition-colors group"
              >
                <div className="flex items-start justify-between mb-3">
                  <div className="min-w-0">
                    <p className="text-sm font-medium text-mesh-text group-hover:text-white truncate">
                      {key}
                    </p>
                    <p className="text-xs text-mesh-muted mt-0.5 truncate">
                      {latest.repo_url ?? 'ZIP upload'}
                    </p>
                  </div>
                  <a
                    href={`/ipfs/${latest.cid}/index.html`}
                    target="_blank"
                    rel="noopener noreferrer"
                    onClick={(e) => e.stopPropagation()}
                    className="text-xs text-mesh-muted hover:text-mesh-text border border-mesh-border rounded-md px-2 py-1 flex-shrink-0"
                  >
                    Visit
                  </a>
                </div>
                {latest.domain && (
                  <p className="text-xs font-mono text-mesh-accent mb-2 truncate">{latest.domain}</p>
                )}
                <div className="flex items-center gap-3 text-xs text-mesh-muted">
                  <StatusBadge status={latest.build_status} />
                  <span>{latest.created_at}</span>
                </div>
              </Link>
            );
          })}
        </div>
      )}
    </div>
  );
}
