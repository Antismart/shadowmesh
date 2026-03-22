import { useEffect, useRef, useState } from 'react';
import { metrics as metricsApi } from '../api/metrics';
import type { MetricsResponse, HealthResponse } from '../api/types';
import MetricCard from '../components/MetricCard';
import LoadingSkeleton from '../components/LoadingSkeleton';

const MAX_HISTORY = 20;

/* ── helpers ─────────────────────────────────────────────────────── */

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function formatUptime(seconds: number): string {
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (d > 0) return `${d}d ${h}h ${m}m`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

function uptimeParts(seconds: number) {
  return {
    days: Math.floor(seconds / 86400),
    hours: Math.floor((seconds % 86400) / 3600),
    minutes: Math.floor((seconds % 3600) / 60),
    seconds: Math.floor(seconds % 60),
  };
}

function timeLabel(index: number, total: number): string {
  const secsAgo = (total - 1 - index) * 5;
  if (secsAgo === 0) return 'now';
  return `-${secsAgo}s`;
}

interface BandwidthPoint {
  bytes: number;
  delta: number;
  ts: number;
}

/* ── sub-components ──────────────────────────────────────────────── */

function LiveDot() {
  return (
    <span className="ml-2 inline-flex items-center gap-1.5">
      <span className="relative flex h-2 w-2">
        <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-mesh-accent opacity-75" />
        <span className="relative inline-flex h-2 w-2 rounded-full bg-mesh-accent" />
      </span>
      <span className="text-xs text-mesh-muted">Live</span>
    </span>
  );
}

function SectionHeading({ children }: { children: React.ReactNode }) {
  return (
    <h2 className="text-sm font-medium text-mesh-muted uppercase tracking-wider mb-3">
      {children}
    </h2>
  );
}

/* ── Bandwidth bar chart ─────────────────────────────────────────── */

function BandwidthChart({ history }: { history: BandwidthPoint[] }) {
  const maxDelta = Math.max(...history.map((p) => p.delta), 1);

  return (
    <div className="border border-mesh-border rounded-lg p-5">
      <div className="flex items-center justify-between mb-4">
        <div>
          <p className="text-sm text-mesh-muted">Bandwidth Over Time</p>
          <p className="text-xs text-mesh-muted mt-0.5">
            Bytes served per 5s interval (last {history.length} samples)
          </p>
        </div>
        {history.length > 0 && (
          <div className="text-right">
            <p className="text-lg font-semibold text-mesh-text">
              {formatBytes(history[history.length - 1].delta)}
            </p>
            <p className="text-xs text-mesh-muted">latest delta</p>
          </div>
        )}
      </div>

      {/* Chart area */}
      <div className="flex items-end gap-[3px] h-36">
        {history.map((point, i) => {
          const heightPct = maxDelta > 0 ? (point.delta / maxDelta) * 100 : 0;
          const isLatest = i === history.length - 1;
          return (
            <div
              key={point.ts}
              className="flex-1 flex flex-col items-center justify-end h-full group relative"
            >
              {/* Tooltip */}
              <div className="absolute bottom-full mb-2 hidden group-hover:flex flex-col items-center z-10">
                <div className="bg-mesh-surface border border-mesh-border rounded px-2 py-1 text-xs text-mesh-text whitespace-nowrap shadow-lg">
                  <p className="font-medium">{formatBytes(point.delta)}</p>
                  <p className="text-mesh-muted">{timeLabel(i, history.length)}</p>
                </div>
              </div>
              {/* Bar */}
              <div
                className={`w-full rounded-t transition-all duration-300 ${
                  isLatest
                    ? 'bg-mesh-accent'
                    : 'bg-mesh-accent/40 group-hover:bg-mesh-accent/70'
                }`}
                style={{
                  height: `${Math.max(heightPct, 2)}%`,
                  minHeight: '2px',
                }}
              />
            </div>
          );
        })}

        {/* Pad empty slots if fewer than MAX_HISTORY */}
        {Array.from({ length: MAX_HISTORY - history.length }).map((_, i) => (
          <div key={`empty-${i}`} className="flex-1 flex items-end h-full">
            <div className="w-full bg-mesh-border/20 rounded-t" style={{ height: '2px' }} />
          </div>
        ))}
      </div>

      {/* X-axis labels */}
      <div className="flex gap-[3px] mt-1">
        {history.length > 0 && (
          <>
            <span className="flex-1 text-[10px] text-mesh-muted text-left">
              {timeLabel(0, history.length)}
            </span>
            <span
              className="text-[10px] text-mesh-muted text-right"
              style={{ flex: history.length - 1 + (MAX_HISTORY - history.length) }}
            >
              now
            </span>
          </>
        )}
      </div>
    </div>
  );
}

/* ── Cache bar ───────────────────────────────────────────────────── */

function CacheBar({ stats }: { stats: MetricsResponse }) {
  const total = stats.cache.total_entries;
  const expired = stats.cache.expired_entries;
  const active = total - expired;
  const hitPct = total > 0 ? (active / total) * 100 : 0;
  const missPct = total > 0 ? (expired / total) * 100 : 0;
  const utilization = stats.cache.max_entries > 0
    ? ((total / stats.cache.max_entries) * 100).toFixed(0)
    : '0';

  return (
    <div className="border border-mesh-border rounded-lg p-5">
      <div className="flex items-center justify-between mb-1">
        <p className="text-sm text-mesh-muted">Cache Hit / Miss Ratio</p>
        <p className="text-xs text-mesh-muted">
          {total} / {stats.cache.max_entries} entries ({utilization}% full)
        </p>
      </div>

      {/* Stacked horizontal bar */}
      <div className="flex h-6 rounded overflow-hidden bg-mesh-border/30 mt-3 mb-2">
        {hitPct > 0 && (
          <div
            className="bg-emerald-500 transition-all duration-500 flex items-center justify-center"
            style={{ width: `${hitPct}%` }}
          >
            {hitPct > 12 && (
              <span className="text-[11px] font-medium text-white">
                {hitPct.toFixed(0)}% active
              </span>
            )}
          </div>
        )}
        {missPct > 0 && (
          <div
            className="bg-mesh-muted/40 transition-all duration-500 flex items-center justify-center"
            style={{ width: `${missPct}%` }}
          >
            {missPct > 12 && (
              <span className="text-[11px] font-medium text-mesh-text/70">
                {missPct.toFixed(0)}% expired
              </span>
            )}
          </div>
        )}
        {total === 0 && (
          <div className="flex-1 flex items-center justify-center">
            <span className="text-[11px] text-mesh-muted">No cache data</span>
          </div>
        )}
      </div>

      {/* Legend */}
      <div className="flex items-center gap-4 text-xs text-mesh-muted">
        <span className="flex items-center gap-1.5">
          <span className="w-2.5 h-2.5 rounded-sm bg-emerald-500" />
          Active entries ({active})
        </span>
        <span className="flex items-center gap-1.5">
          <span className="w-2.5 h-2.5 rounded-sm bg-mesh-muted/40" />
          Expired entries ({expired})
        </span>
      </div>

      {/* Config details */}
      <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 mt-4 pt-4 border-t border-mesh-border">
        <div>
          <p className="text-[11px] text-mesh-muted uppercase tracking-wider">Max Size</p>
          <p className="text-sm font-medium text-mesh-text mt-0.5">
            {stats.config.cache_max_size_mb} MB
          </p>
        </div>
        <div>
          <p className="text-[11px] text-mesh-muted uppercase tracking-wider">TTL</p>
          <p className="text-sm font-medium text-mesh-text mt-0.5">
            {stats.config.cache_ttl_seconds}s
          </p>
        </div>
        <div>
          <p className="text-[11px] text-mesh-muted uppercase tracking-wider">Max Entries</p>
          <p className="text-sm font-medium text-mesh-text mt-0.5">
            {stats.cache.max_entries.toLocaleString()}
          </p>
        </div>
        <div>
          <p className="text-[11px] text-mesh-muted uppercase tracking-wider">Utilization</p>
          <p className="text-sm font-medium text-mesh-text mt-0.5">{utilization}%</p>
        </div>
      </div>
    </div>
  );
}

/* ── IPFS health panel ───────────────────────────────────────────── */

function IpfsHealthPanel({
  health,
  stats,
}: {
  health: HealthResponse;
  stats: MetricsResponse;
}) {
  const connected = stats.config.ipfs_connected;
  const gatewayStatus = health.status;

  // Derive circuit breaker state from error rate
  const errorRate =
    stats.requests_total > 0
      ? stats.requests_error / stats.requests_total
      : 0;
  const circuitState: 'closed' | 'half-open' | 'open' =
    errorRate > 0.5 ? 'open' : errorRate > 0.2 ? 'half-open' : 'closed';

  const circuitColors = {
    closed: 'bg-emerald-500',
    'half-open': 'bg-mesh-warning',
    open: 'bg-mesh-error',
  };

  const circuitLabels = {
    closed: 'Closed (healthy)',
    'half-open': 'Half-open (recovering)',
    open: 'Open (failing)',
  };

  return (
    <div className="border border-mesh-border rounded-lg p-5">
      <p className="text-sm text-mesh-muted mb-4">IPFS & Gateway Health</p>
      <div className="space-y-4">
        {/* IPFS Connection */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2.5">
            <span
              className={`w-2.5 h-2.5 rounded-full ${
                connected ? 'bg-emerald-500' : 'bg-mesh-error'
              }`}
            />
            <span className="text-sm text-mesh-text">IPFS Connection</span>
          </div>
          <span
            className={`text-sm font-medium ${
              connected ? 'text-emerald-400' : 'text-mesh-error'
            }`}
          >
            {connected ? 'Connected' : 'Disconnected'}
          </span>
        </div>

        {/* Gateway status */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2.5">
            <span
              className={`w-2.5 h-2.5 rounded-full ${
                gatewayStatus === 'ok' ? 'bg-emerald-500' : 'bg-mesh-warning'
              }`}
            />
            <span className="text-sm text-mesh-text">Gateway Status</span>
          </div>
          <span
            className={`text-sm font-medium ${
              gatewayStatus === 'ok' ? 'text-emerald-400' : 'text-mesh-warning'
            }`}
          >
            {gatewayStatus === 'ok' ? 'Operational' : gatewayStatus}
          </span>
        </div>

        {/* Circuit Breaker */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2.5">
            <span className={`w-2.5 h-2.5 rounded-full ${circuitColors[circuitState]}`} />
            <span className="text-sm text-mesh-text">Circuit Breaker</span>
          </div>
          <span className="text-sm font-medium text-mesh-text">
            {circuitLabels[circuitState]}
          </span>
        </div>

        {/* Rate Limiting */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2.5">
            <span
              className={`w-2.5 h-2.5 rounded-full ${
                stats.config.rate_limit_enabled ? 'bg-emerald-500' : 'bg-mesh-muted'
              }`}
            />
            <span className="text-sm text-mesh-text">Rate Limiting</span>
          </div>
          <span className="text-sm font-medium text-mesh-text">
            {stats.config.rate_limit_enabled
              ? `${stats.config.rate_limit_rps} req/s`
              : 'Disabled'}
          </span>
        </div>
      </div>
    </div>
  );
}

/* ── Uptime panel ────────────────────────────────────────────────── */

function UptimePanel({ seconds }: { seconds: number }) {
  const parts = uptimeParts(seconds);

  const blocks = [
    { label: 'Days', value: parts.days },
    { label: 'Hours', value: parts.hours },
    { label: 'Minutes', value: parts.minutes },
    { label: 'Seconds', value: parts.seconds },
  ];

  return (
    <div className="border border-mesh-border rounded-lg p-5">
      <div className="flex items-center justify-between mb-4">
        <p className="text-sm text-mesh-muted">Uptime</p>
        <p className="text-xs text-mesh-muted font-mono">{formatUptime(seconds)}</p>
      </div>
      <div className="grid grid-cols-4 gap-3">
        {blocks.map((b) => (
          <div key={b.label} className="text-center">
            <p className="text-2xl font-semibold text-mesh-text font-mono tabular-nums">
              {String(b.value).padStart(2, '0')}
            </p>
            <p className="text-[11px] text-mesh-muted uppercase tracking-wider mt-1">
              {b.label}
            </p>
          </div>
        ))}
      </div>

      {/* Uptime bar visualization */}
      <div className="mt-4 pt-3 border-t border-mesh-border">
        <div className="flex items-center justify-between text-xs text-mesh-muted mb-1.5">
          <span>Uptime progress</span>
          <span>{seconds > 0 ? '99.9%' : '0%'}</span>
        </div>
        <div className="h-1.5 rounded-full bg-mesh-border/50 overflow-hidden">
          <div
            className="h-full rounded-full bg-emerald-500 transition-all duration-1000"
            style={{ width: seconds > 0 ? '99.9%' : '0%' }}
          />
        </div>
      </div>
    </div>
  );
}

/* ── Error rate sparkline ────────────────────────────────────────── */

function ErrorRateIndicator({ stats }: { stats: MetricsResponse }) {
  const errorRate =
    stats.requests_total > 0
      ? (stats.requests_error / stats.requests_total) * 100
      : 0;
  const successRate =
    stats.requests_total > 0
      ? (stats.requests_success / stats.requests_total) * 100
      : 100;

  const statusColor =
    errorRate > 10 ? 'text-mesh-error' : errorRate > 2 ? 'text-mesh-warning' : 'text-emerald-400';
  const barColor =
    errorRate > 10 ? 'bg-mesh-error' : errorRate > 2 ? 'bg-mesh-warning' : 'bg-emerald-500';

  return (
    <div className="border border-mesh-border rounded-lg p-5">
      <div className="flex items-center justify-between mb-3">
        <p className="text-sm text-mesh-muted">Success / Error Rate</p>
        <p className={`text-sm font-medium ${statusColor}`}>
          {errorRate.toFixed(1)}% errors
        </p>
      </div>

      {/* Stacked bar */}
      <div className="flex h-3 rounded-full overflow-hidden bg-mesh-border/30">
        {successRate > 0 && (
          <div
            className="bg-emerald-500 transition-all duration-500"
            style={{ width: `${successRate}%` }}
          />
        )}
        {errorRate > 0 && (
          <div
            className={`${barColor} transition-all duration-500`}
            style={{ width: `${errorRate}%` }}
          />
        )}
      </div>

      <div className="flex items-center justify-between mt-2 text-xs text-mesh-muted">
        <span>{successRate.toFixed(1)}% success ({stats.requests_success.toLocaleString()})</span>
        <span>{errorRate.toFixed(1)}% error ({stats.requests_error.toLocaleString()})</span>
      </div>
    </div>
  );
}

/* ── Main page ───────────────────────────────────────────────────── */

export default function AnalyticsPage() {
  const [stats, setStats] = useState<MetricsResponse | null>(null);
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null);

  // Bandwidth history: track last MAX_HISTORY data points
  const prevBytesRef = useRef<number | null>(null);
  const [bandwidthHistory, setBandwidthHistory] = useState<BandwidthPoint[]>([]);

  const fetchData = () => {
    Promise.all([
      metricsApi.stats().catch(() => null),
      metricsApi.health().catch(() => null),
    ]).then(([s, h]) => {
      if (s) {
        setStats(s);

        // Compute bandwidth delta
        const now = Date.now();
        const prevBytes = prevBytesRef.current;
        const delta = prevBytes !== null ? Math.max(0, s.bytes_served - prevBytes) : 0;
        prevBytesRef.current = s.bytes_served;

        // Only record after the first poll (so we have a real delta)
        if (prevBytes !== null) {
          setBandwidthHistory((prev) => {
            const next = [...prev, { bytes: s.bytes_served, delta, ts: now }];
            return next.slice(-MAX_HISTORY);
          });
        }
      }
      if (h) setHealth(h);
      setLastUpdated(new Date());
      setLoading(false);
    });
  };

  useEffect(() => {
    fetchData();
    const interval = setInterval(fetchData, 5000);
    return () => clearInterval(interval);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (loading) return <LoadingSkeleton count={6} />;

  const successRate =
    stats && stats.requests_total > 0
      ? ((stats.requests_success / stats.requests_total) * 100).toFixed(1)
      : '100.0';

  const errorRate =
    stats && stats.requests_total > 0
      ? ((stats.requests_error / stats.requests_total) * 100).toFixed(1)
      : '0.0';

  return (
    <div>
      {/* Header */}
      <div className="mb-8 flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-semibold text-mesh-text">Analytics</h1>
          <p className="text-sm text-mesh-muted mt-1">
            Real-time gateway metrics
            <LiveDot />
          </p>
        </div>
        {lastUpdated && (
          <p className="text-xs text-mesh-muted mt-1">
            Updated {lastUpdated.toLocaleTimeString()}
          </p>
        )}
      </div>

      {/* ── Request metrics ─────────────────────────────────────── */}
      <SectionHeading>Requests</SectionHeading>
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4 mb-6">
        <MetricCard
          label="Total Requests"
          value={(stats?.requests_total ?? 0).toLocaleString()}
        />
        <MetricCard
          label="Successful"
          value={(stats?.requests_success ?? 0).toLocaleString()}
          sub={`${successRate}% success rate`}
        />
        <MetricCard
          label="Errors"
          value={(stats?.requests_error ?? 0).toLocaleString()}
          sub={`${errorRate}% error rate`}
        />
        <MetricCard
          label="Bandwidth Served"
          value={formatBytes(stats?.bytes_served ?? 0)}
        />
      </div>

      {/* ── Error rate bar ──────────────────────────────────────── */}
      {stats && (
        <div className="mb-6">
          <ErrorRateIndicator stats={stats} />
        </div>
      )}

      {/* ── Bandwidth chart ─────────────────────────────────────── */}
      <SectionHeading>Bandwidth</SectionHeading>
      <div className="mb-6">
        <BandwidthChart history={bandwidthHistory} />
      </div>

      {/* ── Cache performance ───────────────────────────────────── */}
      <SectionHeading>Cache Performance</SectionHeading>
      <div className="mb-6">{stats && <CacheBar stats={stats} />}</div>

      {/* ── System: IPFS health + Uptime side by side ───────────── */}
      <SectionHeading>System</SectionHeading>
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4 mb-6">
        {health && stats && <IpfsHealthPanel health={health} stats={stats} />}
        <UptimePanel seconds={health?.uptime_seconds ?? stats?.uptime_seconds ?? 0} />
      </div>

      {/* ── Footer: version ─────────────────────────────────────── */}
      <div className="mt-4 pt-4 border-t border-mesh-border flex items-center justify-between text-xs text-mesh-muted">
        <span>ShadowMesh Gateway v{health?.version ?? '-'}</span>
        <span>Polling every 5s</span>
      </div>
    </div>
  );
}
