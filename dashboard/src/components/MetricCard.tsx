interface MetricCardProps {
  label: string;
  value: string | number;
  sub?: string;
  accent?: boolean;
}

export default function MetricCard({ label, value, sub }: MetricCardProps) {
  return (
    <div className="border border-mesh-border rounded-lg p-5">
      <p className="text-sm text-mesh-muted mb-1">{label}</p>
      <p className="text-2xl font-semibold text-mesh-text">{value}</p>
      {sub && <p className="text-xs text-mesh-muted mt-1">{sub}</p>}
    </div>
  );
}
