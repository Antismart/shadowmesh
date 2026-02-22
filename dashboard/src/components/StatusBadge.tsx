const dotColors: Record<string, string> = {
  Ready: 'bg-[#0070f3]',
  Built: 'bg-[#0070f3]',
  Uploaded: 'bg-[#0070f3]',
  Failed: 'bg-[#ee0000]',
  Building: 'bg-[#f5a623]',
  Skipped: 'bg-mesh-muted',
};

export default function StatusBadge({ status }: { status: string }) {
  const dot = dotColors[status] ?? dotColors.Skipped;
  return (
    <span className="inline-flex items-center gap-2 text-sm text-mesh-muted">
      <span className={`w-2.5 h-2.5 rounded-full flex-shrink-0 ${dot}`} />
      {status}
    </span>
  );
}
