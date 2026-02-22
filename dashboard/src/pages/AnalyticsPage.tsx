import { useEffect, useState } from 'react';
import { metrics as metricsApi } from '../api/metrics';
import type { MetricsResponse, HealthResponse } from '../api/types';
import MetricCard from '../components/MetricCard';
import LoadingSkeleton from '../components/LoadingSkeleton';

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

export default function AnalyticsPage() {
  const [stats, setStats] = useState<MetricsResponse | null>(null);
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [loading, setLoading] = useState(true);

  const fetchData = () => {
    Promise.all([
      metricsApi.stats().catch(() => null),
      metricsApi.health().catch(() => null),
    ]).then(([s, h]) => {
      if (s) setStats(s);
      if (h) setHealth(h);
      setLoading(false);
    });
  };

  useEffect(() => {
    fetchData();
    const interval = setInterval(fetchData, 5000);
    return () => clearInterval(interval);
  }, []);

  if (loading) return <LoadingSkeleton count={4} />;

  const successRate = stats && stats.requests_total > 0
    ? ((stats.requests_success / stats.requests_total) * 100).toFixed(1)
    : '100';

  const cacheHitRate = stats?.cache && stats.cache.total_entries > 0
    ? ((1 - stats.cache.expired_entries / stats.cache.total_entries) * 100).toFixed(0)
    : '-';

  return (
    <div>
      <div className="mb-8">
        <h1 className="text-2xl font-semibold text-mesh-text">Usage</h1>
        <p className="text-sm text-mesh-muted mt-1">
          Real-time gateway metrics
          <span className="ml-2 inline-flex items-center gap-1">
            <span className="w-1.5 h-1.5 rounded-full bg-mesh-accent animate-pulse" />
            <span className="text-xs">Live</span>
          </span>
        </p>
      </div>

      {/* Request metrics */}
      <h2 className="text-sm font-medium text-mesh-muted uppercase tracking-wider mb-3">Requests</h2>
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4 mb-8">
        <MetricCard label="Total Requests" value={stats?.requests_total ?? 0} />
        <MetricCard label="Successful" value={stats?.requests_success ?? 0} sub={`${successRate}% success rate`} />
        <MetricCard label="Errors" value={stats?.requests_error ?? 0} />
        <MetricCard label="Bandwidth Served" value={formatBytes(stats?.bytes_served ?? 0)} />
      </div>

      {/* Cache metrics */}
      <h2 className="text-sm font-medium text-mesh-muted uppercase tracking-wider mb-3">Cache</h2>
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4 mb-8">
        <MetricCard label="Cache Entries" value={stats?.cache.total_entries ?? 0} sub={`of ${stats?.cache.max_entries ?? 0} max`} />
        <MetricCard label="Hit Rate" value={`${cacheHitRate}%`} />
        <MetricCard label="Max Size" value={`${stats?.config.cache_max_size_mb ?? 0} MB`} />
        <MetricCard label="TTL" value={`${stats?.config.cache_ttl_seconds ?? 0}s`} />
      </div>

      {/* System metrics */}
      <h2 className="text-sm font-medium text-mesh-muted uppercase tracking-wider mb-3">System</h2>
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
        <MetricCard label="Uptime" value={formatUptime(health?.uptime_seconds ?? 0)} />
        <MetricCard
          label="IPFS Status"
          value={stats?.config.ipfs_connected ? 'Connected' : 'Disconnected'}
        />
        <MetricCard
          label="Rate Limiting"
          value={stats?.config.rate_limit_enabled ? 'Enabled' : 'Disabled'}
          sub={stats?.config.rate_limit_enabled ? `${stats?.config.rate_limit_rps} req/s` : undefined}
        />
        <MetricCard label="Version" value={health?.version ?? '-'} />
      </div>
    </div>
  );
}
