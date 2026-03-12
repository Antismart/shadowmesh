import { useEffect, useRef } from 'react';

interface BuildLogViewerProps {
  logs: string;
  status: string;
  streamingLines?: string[];
  isStreaming?: boolean;
}

export default function BuildLogViewer({ logs, status, streamingLines, isStreaming }: BuildLogViewerProps) {
  const preRef = useRef<HTMLPreElement>(null);

  useEffect(() => {
    if (streamingLines && preRef.current) {
      preRef.current.scrollTop = preRef.current.scrollHeight;
    }
  }, [streamingLines]);

  const displayText = streamingLines
    ? streamingLines.join('\n')
    : logs;

  return (
    <div className="border border-mesh-border rounded-lg overflow-hidden">
      <div className="flex items-center justify-between px-4 py-3 border-b border-mesh-border bg-mesh-surface">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium text-mesh-text">Build Output</span>
          {isStreaming && (
            <span className="relative flex h-2 w-2">
              <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-mesh-accent opacity-75" />
              <span className="relative inline-flex rounded-full h-2 w-2 bg-mesh-accent" />
            </span>
          )}
        </div>
        <span
          className={`text-xs font-medium ${
            status === 'Built' || status === 'Ready' || status === 'success'
              ? 'text-mesh-accent'
              : status === 'Failed' || status === 'error'
                ? 'text-[#ee0000]'
                : 'text-mesh-muted'
          }`}
        >
          {isStreaming ? 'Building...' : status}
        </span>
      </div>
      <pre
        ref={preRef}
        className="p-4 text-xs font-mono text-mesh-muted leading-relaxed overflow-x-auto max-h-96 overflow-y-auto bg-mesh-bg"
      >
        {displayText || 'No build logs available.'}
      </pre>
    </div>
  );
}
