# Alert: ShadowMeshIPFSCircuitOpen

## Summary
Triggers when the IPFS circuit breaker has opened due to repeated failures.

## Severity
High

## Impact
- All IPFS operations will fail fast with 503 Service Unavailable
- New deployments cannot be created
- Content retrieval may fail (if not cached)

## Investigation Steps

1. **Check gateway logs for circuit breaker events**
   ```bash
   kubectl logs -l app=shadowmesh-gateway --tail=200 | grep -i "circuit"
   ```

2. **Verify IPFS daemon status**
   ```bash
   kubectl get pods -l app=ipfs
   kubectl logs -l app=ipfs --tail=100
   ```

3. **Test IPFS API connectivity**
   ```bash
   kubectl exec -it deploy/shadowmesh-gateway -- curl -s http://ipfs:5001/api/v0/id
   ```

4. **Check IPFS peer connections**
   ```bash
   kubectl exec -it deploy/ipfs -- ipfs swarm peers | wc -l
   ```

5. **Review Prometheus metrics**
   ```promql
   shadowmesh_ipfs_errors_total
   shadowmesh_circuit_breaker_state
   ```

## Resolution Steps

1. **Wait for automatic recovery**
   - Circuit breaker will attempt reset after 30 seconds (default)
   - Monitor logs for "Circuit half-open" then "Circuit closed"

2. **If IPFS is unresponsive:**
   ```bash
   kubectl rollout restart deployment/ipfs
   ```

3. **If IPFS is overwhelmed:**
   - Check disk usage: `kubectl exec -it deploy/ipfs -- df -h`
   - Check memory: `kubectl top pod -l app=ipfs`
   - Consider scaling IPFS or adding resource limits

4. **If network partition:**
   - Verify NetworkPolicy allows gateway -> IPFS traffic
   - Check Service endpoints: `kubectl get endpoints ipfs`

5. **Manual circuit breaker reset (if stuck):**
   ```bash
   kubectl rollout restart deployment/shadowmesh-gateway
   ```

## Escalation
If IPFS cannot be restored within 15 minutes, escalate to infrastructure team.

## Prevention
- Configure appropriate circuit breaker thresholds for your environment
- Monitor IPFS health proactively
- Set up IPFS clustering for redundancy
