# Alert: ShadowMeshRedisDisconnected

## Summary
Triggers when the gateway cannot connect to Redis or Redis operations are failing.

## Severity
High

## Impact
- Deployment data not persisted (will be lost on restart)
- Rate limiting falls back to local (won't work across pods)
- API key validation falls back to in-memory cache

## Investigation Steps

1. **Check Redis connectivity from gateway pod**
   ```bash
   kubectl exec -it deploy/shadowmesh-gateway -- nc -zv redis 6379
   ```

2. **Check Redis pod status**
   ```bash
   kubectl get pods -l app=redis
   kubectl describe pod -l app=redis
   ```

3. **Query gateway logs for Redis errors**
   ```bash
   kubectl logs -l app=shadowmesh-gateway --tail=200 | grep -i redis
   ```

4. **Check Redis credentials**
   ```bash
   kubectl get secret redis-credentials -o yaml
   ```

5. **Verify REDIS_URL environment variable**
   ```bash
   kubectl exec -it deploy/shadowmesh-gateway -- printenv | grep REDIS
   ```

## Resolution Steps

1. **If Redis pod is down:**
   ```bash
   kubectl rollout restart statefulset/redis
   ```

2. **If network issue:**
   - Check NetworkPolicy allows gateway -> redis traffic
   - Verify Service DNS resolution:
     ```bash
     kubectl exec -it deploy/shadowmesh-gateway -- nslookup redis
     ```

3. **If credentials expired/wrong:**
   - Update the redis-credentials Secret
   - Restart gateway to pick up new credentials

4. **If Redis memory exhausted:**
   ```bash
   kubectl exec -it redis-0 -- redis-cli INFO memory
   ```
   - Increase memory limit or enable eviction policy

## Escalation
If Redis cannot be restored within 10 minutes, notify the platform team. Gateway will continue operating with in-memory fallback but data durability is compromised.

## Prevention
- Set up Redis Sentinel or Cluster for high availability
- Monitor Redis memory usage with alerts at 80% threshold
- Regular backup of Redis data
