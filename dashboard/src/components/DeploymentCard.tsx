import { Link } from 'react-router-dom';
import type { Deployment } from '../api/types';
import StatusBadge from './StatusBadge';

export default function DeploymentCard({ deployment }: { deployment: Deployment }) {
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

      <button
        onClick={(e) => e.preventDefault()}
        className="text-mesh-muted hover:text-mesh-text opacity-0 group-hover:opacity-100 transition-opacity p-1"
      >
        <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 16 16">
          <circle cx="8" cy="3" r="1.5" />
          <circle cx="8" cy="8" r="1.5" />
          <circle cx="8" cy="13" r="1.5" />
        </svg>
      </button>
    </Link>
  );
}
