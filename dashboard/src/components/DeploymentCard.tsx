import { Link } from 'react-router-dom';
import type { Deployment } from '../api/types';
import StatusBadge from './StatusBadge';

interface DeploymentCardProps {
  deployment: Deployment;
  onRedeploy?: (cid: string) => void;
}

export default function DeploymentCard({ deployment, onRedeploy }: DeploymentCardProps) {
  return (
    <Link
      to={`/deployments/${deployment.cid}`}
      className="flex items-center gap-4 px-4 py-3 border-b border-mesh-border last:border-b-0 hover:bg-mesh-surface transition-colors group"
    >
      <StatusBadge status={deployment.build_status} />

      <div className="flex-1 min-w-0">
        <span className="text-sm text-mesh-text group-hover:text-white truncate">
          {deployment.name}
        </span>
        <span className="text-sm text-mesh-muted ml-2 font-mono">
          {deployment.cid.slice(0, 8)}
        </span>
      </div>

      {deployment.branch && (
        <span className="text-sm text-mesh-muted hidden sm:block font-mono">
          {deployment.branch}
        </span>
      )}

      <span className="text-sm text-mesh-muted flex-shrink-0">
        {deployment.created_at}
      </span>

      {deployment.source === 'github' && onRedeploy && (
        <button
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            onRedeploy(deployment.cid);
          }}
          className="text-mesh-muted hover:text-mesh-accent opacity-0 group-hover:opacity-100 transition-opacity p-1"
          title="Redeploy"
        >
          <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182" />
          </svg>
        </button>
      )}
    </Link>
  );
}
