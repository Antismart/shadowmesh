# Alert: ShadowMeshHighLatency / ShadowMeshVeryHighLatency

## Summary
- **Warning**: P95 latency exceeds 1 second for 5+ minutes
- **Critical**: P95 latency exceeds 5 seconds for 2+ minutes

## Severity
Warning / Critical

## Impact
Users experiencing slow page loads and API responses. Timeouts may occur for long operations.

## Investigation Steps

1. **Check the Grafana dashboard**
   - Navigate to: ShadowMesh Gateway dashboard
   - Look at "Response Time Distribution" panel
   - Identify which percentiles are affected (p50, p90, p95, p99)

2. **Check resource utilization**
   ```bash
   kubectl top pods -l app=shadowmesh-gateway
   ```

3. **Check IPFS operation latency**
   ```promql
   histogram_quantile(0.95, sum(rate(ipfs_operation_duration_seconds_bucket[5m])) by (le, operation))
   ```

4. **Check cache hit rate**
   ```promql
   sum(rate(cache_hits_total[5m])) / (sum(rate(cache_hits_total[5m])) + sum(rate(cache_misses_total[5m])))
   ```

5. **Look for slow queries in logs**
   ```bash
   kubectl logs -l app=shadowmesh-gateway --tail=500 | grep -E "duration_ms=[0-9]{4,}"
   ```

## Resolution Steps

1. **If IPFS is slow:**
   - Check IPFS daemon health and peers
   - Consider adding more IPFS nodes
   - Review content pinning strategy

2. **If cache hit rate is low:**
   - Increase cache size in configuration
   - Check if TTL is too short
   - Look for cache invalidation patterns

3. **If CPU is saturated:**
   - Scale horizontally: `kubectl scale deployment/shadowmesh-gateway --replicas=5`
   - Check for CPU-intensive operations
   - Review recent code changes

4. **If memory pressure:**
   - Check for memory leaks
   - Increase memory limits
   - Review cache memory usage

5. **If network latency:**
   - Check network policies
   - Verify DNS resolution times
   - Check for network saturation

## Escalation
- Warning: Monitor for 30 minutes, escalate if no improvement
- Critical: Escalate immediately to platform team

## Prevention
- Set appropriate resource requests and limits
- Implement proper caching strategy
- Use CDN for static content where possible
- Monitor latency trends proactively
