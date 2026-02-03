# Alert: ShadowMeshDeploymentPersistFailed

## Summary
Triggers when deployment metadata fails to persist to Redis.

## Severity
Medium

## Impact
- Deployments created successfully but metadata may be lost on restart
- Dashboard may show stale data after gateway restart
- In-memory state is still accurate until restart

## Investigation Steps

1. **Check gateway logs for persistence errors**
   ```bash
   kubectl logs -l app=shadowmesh-gateway --tail=200 | grep -i "persist\|redis\|deployment"
   ```

2. **Verify Redis connectivity**
   ```bash
   kubectl exec -it deploy/shadowmesh-gateway -- nc -zv redis 6379
   ```

3. **Check Redis key space**
   ```bash
   kubectl exec -it redis-0 -- redis-cli KEYS "shadowmesh:deployments:*"
   ```

4. **Verify deployment sorted set**
   ```bash
   kubectl exec -it redis-0 -- redis-cli ZRANGE shadowmesh:deployment_list 0 -1
   ```

5. **Check Redis memory**
   ```bash
   kubectl exec -it redis-0 -- redis-cli INFO memory
   ```

## Resolution Steps

1. **If Redis is full:**
   - Check max memory setting
   - Clear old/unused deployment data:
     ```bash
     kubectl exec -it redis-0 -- redis-cli ZREMRANGEBYRANK shadowmesh:deployment_list 0 -100
     ```

2. **If network issue:**
   - See [redis-disconnected.md](./redis-disconnected.md) runbook

3. **If serialization error:**
   - Check for corrupted deployment data in logs
   - May need to clear specific keys and re-deploy

4. **Re-sync in-memory to Redis:**
   - Currently requires gateway restart
   - All in-memory deployments will be persisted on creation

## Escalation
If persistence failures continue for more than 30 minutes, notify the platform team.

## Prevention
- Monitor Redis memory usage
- Set appropriate TTL on deployment keys if needed
- Regular Redis backups
