export default function LoadingSkeleton({
  count = 3,
  className = '',
}: {
  count?: number;
  className?: string;
}) {
  return (
    <div className={`border border-mesh-border rounded-lg overflow-hidden ${className}`}>
      {Array.from({ length: count }).map((_, i) => (
        <div key={i} className="flex items-center gap-4 px-4 py-3 border-b border-mesh-border last:border-b-0 animate-pulse">
          <div className="w-2.5 h-2.5 rounded-full bg-mesh-border" />
          <div className="flex-1 space-y-1.5">
            <div className="h-3.5 w-1/3 rounded bg-mesh-border" />
            <div className="h-3 w-1/4 rounded bg-mesh-border/50" />
          </div>
          <div className="h-3 w-16 rounded bg-mesh-border/50" />
        </div>
      ))}
    </div>
  );
}
