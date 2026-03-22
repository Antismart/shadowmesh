import { useEffect, useState } from 'react';
import { useNavigate, Link } from 'react-router-dom';
import { deployments as deploymentsApi } from '../api/deployments';
import { metrics as metricsApi } from '../api/metrics';
import type { Deployment, MetricsResponse } from '../api/types';
import StatusBadge from '../components/StatusBadge';
import MetricCard from '../components/MetricCard';
import LoadingSkeleton from '../components/LoadingSkeleton';
import GithubLoginLink from '../components/GithubLoginLink';
import { useAuth } from '../context/AuthContext';

/* ------------------------------------------------------------------ */
/*  Helpers                                                           */
/* ------------------------------------------------------------------ */

/** Format bytes into a human-readable string (KB / MB / GB). */
function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

/** Turn an ISO timestamp into a relative label like "2h ago". */
function timeAgo(iso: string): string {
  const seconds = Math.floor((Date.now() - new Date(iso).getTime()) / 1000);
  if (seconds < 60) return 'just now';
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d ago`;
  const months = Math.floor(days / 30);
  return `${months}mo ago`;
}

/* ------------------------------------------------------------------ */
/*  Sub-components                                                    */
/* ------------------------------------------------------------------ */

function StatsRow({
  deploymentsList,
  metricsData,
}: {
  deploymentsList: Deployment[];
  metricsData: MetricsResponse | null;
}) {
  const uniqueRepos = new Set(
    deploymentsList.map((d) => d.repo_url?.split('/').pop() ?? d.name),
  );
  const activeDomains = deploymentsList.filter((d) => d.domain !== null).length;
  const bytesServed = metricsData?.bytes_served ?? 0;

  return (
    <div className="grid grid-cols-2 lg:grid-cols-4 gap-4 mb-8">
      <MetricCard label="Total Projects" value={uniqueRepos.size} />
      <MetricCard label="Total Deployments" value={deploymentsList.length} />
      <MetricCard label="Bandwidth Served" value={formatBytes(bytesServed)} />
      <MetricCard label="Active Domains" value={activeDomains} />
    </div>
  );
}

function RecentDeployments({ deployments }: { deployments: Deployment[] }) {
  const sorted = [...deployments]
    .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
    .slice(0, 5);

  if (sorted.length === 0) return null;

  return (
    <section className="mb-8">
      <h2 className="text-sm font-medium text-mesh-muted uppercase tracking-wider mb-3">
        Recent Deployments
      </h2>
      <div className="border border-mesh-border rounded-lg divide-y divide-mesh-border">
        {sorted.map((d) => (
          <Link
            key={`${d.name}-${d.cid}`}
            to={`/deployments/${d.name}`}
            className="flex items-center justify-between px-4 py-3 hover:bg-mesh-surface-hover transition-colors first:rounded-t-lg last:rounded-b-lg"
          >
            <div className="flex items-center gap-3 min-w-0">
              <span className="text-sm text-mesh-text truncate font-medium">
                {d.name}
              </span>
              <StatusBadge status={d.build_status} />
            </div>
            <span className="text-xs text-mesh-muted flex-shrink-0 ml-4">
              {timeAgo(d.created_at)}
            </span>
          </Link>
        ))}
      </div>
    </section>
  );
}

function QuickActions() {
  const navigate = useNavigate();

  return (
    <section className="mb-8">
      <h2 className="text-sm font-medium text-mesh-muted uppercase tracking-wider mb-3">
        Quick Actions
      </h2>
      <div className="flex flex-wrap gap-3">
        <button onClick={() => navigate('/new')} className="btn-secondary inline-flex items-center gap-2">
          <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M12 4v16m8-8H4" />
          </svg>
          Import Project
        </button>
        <button onClick={() => navigate('/domains')} className="btn-secondary inline-flex items-center gap-2">
          <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M21 12a9 9 0 01-9 9m9-9a9 9 0 00-9-9m9 9H3m9 9a9 9 0 01-9-9m9 9c1.657 0 3-4.03 3-9s-1.343-9-3-9m0 18c-1.657 0-3-4.03-3-9s1.343-9 3-9" />
          </svg>
          Manage Domains
        </button>
        <button onClick={() => navigate('/analytics')} className="btn-secondary inline-flex items-center gap-2">
          <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M9 19v-6a2 2 0 00-2-2H5a2 2 0 00-2 2v6a2 2 0 002 2h2a2 2 0 002-2zm0 0V9a2 2 0 012-2h2a2 2 0 012 2v10m-6 0a2 2 0 002 2h2a2 2 0 002-2m0 0V5a2 2 0 012-2h2a2 2 0 012 2v14a2 2 0 01-2 2h-2a2 2 0 01-2-2z" />
          </svg>
          View Analytics
        </button>
      </div>
    </section>
  );
}

/* ------------------------------------------------------------------ */
/*  Onboarding checklist                                              */
/* ------------------------------------------------------------------ */

interface OnboardingStepProps {
  step: number;
  title: string;
  completed: boolean;
  action: React.ReactNode;
}

function OnboardingStep({ step, title, completed, action }: OnboardingStepProps) {
  return (
    <div className="flex items-center gap-4 px-5 py-4 border border-mesh-border rounded-lg">
      {/* Step number / check */}
      <div
        className={`w-8 h-8 rounded-full flex items-center justify-center flex-shrink-0 text-sm font-semibold ${
          completed
            ? 'bg-green-500/15 text-green-400 border border-green-500/30'
            : 'bg-mesh-surface border border-mesh-border text-mesh-muted'
        }`}
      >
        {completed ? (
          <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={3}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" />
          </svg>
        ) : (
          step
        )}
      </div>

      {/* Title */}
      <span
        className={`text-sm flex-1 ${
          completed ? 'text-mesh-muted line-through' : 'text-mesh-text font-medium'
        }`}
      >
        {title}
      </span>

      {/* Action */}
      {!completed && <div className="flex-shrink-0">{action}</div>}
    </div>
  );
}

function OnboardingChecklist({ connected }: { connected: boolean }) {
  const navigate = useNavigate();

  return (
    <section>
      <div className="mb-6">
        <h2 className="text-xl font-semibold text-mesh-text mb-1">Get started with ShadowMesh</h2>
        <p className="text-sm text-mesh-muted">
          Complete these steps to deploy your first project on the decentralized web.
        </p>
      </div>

      <div className="flex flex-col gap-3 max-w-xl">
        <OnboardingStep
          step={1}
          title="Connect GitHub"
          completed={connected}
          action={
            <GithubLoginLink className="btn-primary text-sm inline-flex items-center gap-2">
              <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 24 24">
                <path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z" />
              </svg>
              Connect
            </GithubLoginLink>
          }
        />
        <OnboardingStep
          step={2}
          title="Deploy your first project"
          completed={false}
          action={
            <button onClick={() => navigate('/new')} className="btn-primary text-sm">
              Deploy
            </button>
          }
        />
        <OnboardingStep
          step={3}
          title="Assign a custom domain"
          completed={false}
          action={
            <button onClick={() => navigate('/domains')} className="btn-primary text-sm">
              Domains
            </button>
          }
        />
        <OnboardingStep
          step={4}
          title="Explore analytics"
          completed={false}
          action={
            <button onClick={() => navigate('/analytics')} className="btn-primary text-sm">
              Analytics
            </button>
          }
        />
      </div>
    </section>
  );
}

/* ------------------------------------------------------------------ */
/*  Main page                                                         */
/* ------------------------------------------------------------------ */

export default function OverviewPage() {
  const navigate = useNavigate();
  const { connected } = useAuth();

  const [deploymentsList, setDeployments] = useState<Deployment[]>([]);
  const [metricsData, setMetricsData] = useState<MetricsResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [search, setSearch] = useState('');

  useEffect(() => {
    Promise.all([
      deploymentsApi.list().catch(() => [] as Deployment[]),
      metricsApi.stats().catch(() => null),
    ]).then(([deps, met]) => {
      setDeployments(deps);
      setMetricsData(met);
      setLoading(false);
    });
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

  const hasDeployments = deploymentsList.length > 0;

  return (
    <div>
      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-semibold text-mesh-text">Projects</h1>
        <button onClick={() => navigate('/new')} className="btn-primary">
          Add New...
        </button>
      </div>

      {loading ? (
        <LoadingSkeleton count={4} />
      ) : !hasDeployments ? (
        /* ---------- Onboarding (empty state) ---------- */
        <OnboardingChecklist connected={connected} />
      ) : (
        /* ---------- Dashboard with data ---------- */
        <>
          {/* Stats cards */}
          <StatsRow deploymentsList={deploymentsList} metricsData={metricsData} />

          {/* Recent deployments */}
          <RecentDeployments deployments={deploymentsList} />

          {/* Quick actions */}
          <QuickActions />

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

          {/* Project grid */}
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
        </>
      )}
    </div>
  );
}
