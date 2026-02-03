# Alert: ShadowMeshHighErrorRate

## Summary
Triggers when the 5xx error rate exceeds 5% of total requests for more than 5 minutes.

## Severity
Critical

## Impact
Users are experiencing service failures. Deployments may fail, content may be inaccessible.

## Investigation Steps

1. **Check the Grafana dashboard**
   - Navigate to: ShadowMesh Gateway dashboard
   - Look at the "Error Rate" panel for spike patterns

2. **Query recent logs**
   ```bash
   kubectl logs -l app=shadowmesh-gateway --tail=200 | grep -i error
   ```

3. **Check IPFS connectivity**
   ```bash
   kubectl exec -it deploy/shadowmesh-gateway -- curl -s http://localhost:8081/health | jq .ipfs_connected
   ```

4. **Check circuit breaker status**
   - Look for "Circuit opened" log messages
   - Check if IPFS operations are failing

5. **Review Prometheus metrics**
   ```promql
   sum(rate(http_requests_total{status=~"5.."}[5m])) by (endpoint)
   ```

## Resolution Steps

1. **If IPFS is disconnected:**
   - Restart the IPFS daemon
   - Check network connectivity between gateway and IPFS
   - Verify IPFS configuration in ConfigMap

2. **If circuit breaker is open:**
   - Wait for automatic recovery (30s default)
   - Check underlying IPFS service health
   - Consider increasing circuit breaker thresholds if transient

3. **If memory/CPU exhausted:**
   - Check pod resource usage: `kubectl top pods`
   - Scale horizontally if needed
   - Check for memory leaks in recent deployments

4. **If due to bad deployment:**
   - Roll back to previous version:
     ```bash
     kubectl rollout undo deployment/shadowmesh-gateway
     ```

## Escalation
If unresolved after 15 minutes, escalate to the on-call platform engineer.

## Prevention
- Ensure proper health checks before production deployment
- Monitor error rate trends after each release
- Set up canary deployments for gradual rollout
