import { Link } from 'react-router-dom';

export default function NotFoundPage() {
  return (
    <div className="flex items-center justify-center min-h-[60vh]">
      <div className="text-center">
        <p className="text-6xl font-bold text-mesh-text mb-4">404</p>
        <h1 className="text-xl font-semibold text-mesh-text mb-2">Page not found</h1>
        <p className="text-sm text-mesh-muted mb-6">The page you're looking for doesn't exist.</p>
        <Link to="/" className="btn-primary">
          Back to Projects
        </Link>
      </div>
    </div>
  );
}
