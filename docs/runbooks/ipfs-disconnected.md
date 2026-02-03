# Alert: ShadowMeshIPFSDisconnected

## Summary
The gateway has lost connection to the IPFS daemon for more than 2 minutes.

## Severity
Critical

## Impact
- New deployments will fail
- Content retrieval from IPFS will fail
- Only cached content will be accessible

## Investigation Steps

1. **Check IPFS daemon status**
   ```bash
   kubectl get pods -l app=ipfs
   kubectl logs -l app=ipfs --tail=100
   ```

2. **Check gateway health endpoint**
   ```bash
   kubectl exec -it deploy/shadowmesh-gateway -- curl -s http://localhost:8081/health
   ```

3. **Test IPFS API connectivity**
   ```bash
   kubectl exec -it deploy/shadowmesh-gateway -- curl -s http://ipfs-service:5001/api/v0/id
   ```

4. **Check network connectivity**
   ```bash
   kubectl exec -it deploy/shadowmesh-gateway -- nc -zv ipfs-service 5001
   ```

5. **Review IPFS metrics**
   ```promql
   ipfs_connected
   rate(ipfs_operations_total{status="error"}[5m])
   ```

## Resolution Steps

1. **If IPFS pod is not running:**
   ```bash
   kubectl rollout restart deployment/ipfs
   kubectl rollout status deployment/ipfs
   ```

2. **If IPFS is running but unresponsive:**
   - Check IPFS daemon logs for errors
   - Verify IPFS repository isn't corrupted
   - Check disk space on IPFS volume

3. **If network issue:**
   - Verify Service and Endpoints:
     ```bash
     kubectl get svc ipfs-service
     kubectl get endpoints ipfs-service
     ```
   - Check NetworkPolicy if applicable
   - Verify DNS resolution

4. **If configuration mismatch:**
   - Verify IPFS API URL in ConfigMap:
     ```bash
     kubectl get configmap shadowmesh-config -o yaml
     ```
   - Ensure port matches IPFS configuration

5. **Restart gateway pods (last resort):**
   ```bash
   kubectl rollout restart deployment/shadowmesh-gateway
   ```

## Escalation
Escalate immediately if IPFS cannot be restored within 10 minutes.

## Prevention
- Implement IPFS health monitoring
- Use IPFS cluster for redundancy
- Set up automatic IPFS daemon restart
- Monitor IPFS resource usage
