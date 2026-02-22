interface EmptyStateProps {
  icon?: string;
  title: string;
  description: string;
  action?: { label: string; onClick: () => void };
}

export default function EmptyState({ title, description, action }: EmptyStateProps) {
  return (
    <div className="border border-mesh-border rounded-lg p-16 text-center">
      <h3 className="text-base font-medium text-mesh-text mb-2">{title}</h3>
      <p className="text-sm text-mesh-muted mb-6 max-w-md mx-auto">{description}</p>
      {action && (
        <button onClick={action.onClick} className="btn-primary">
          {action.label}
        </button>
      )}
    </div>
  );
}
