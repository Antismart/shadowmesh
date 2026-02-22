interface BuildLogViewerProps {
  logs: string;
  status: string;
}

export default function BuildLogViewer({ logs, status }: BuildLogViewerProps) {
  return (
    <div className="border border-mesh-border rounded-lg overflow-hidden">
      <div className="flex items-center justify-between px-4 py-3 border-b border-mesh-border bg-mesh-surface">
        <span className="text-sm font-medium text-mesh-text">Build Output</span>
        <span
          className={`text-xs font-medium ${
            status === 'Built' || status === 'Ready'
              ? 'text-mesh-accent'
              : status === 'Failed'
                ? 'text-[#ee0000]'
                : 'text-mesh-muted'
          }`}
        >
          {status}
        </span>
      </div>
      <pre className="p-4 text-xs font-mono text-mesh-muted leading-relaxed overflow-x-auto max-h-96 overflow-y-auto bg-mesh-bg">
        {logs || 'No build logs available.'}
      </pre>
    </div>
  );
}
