# Alert: ShadowMeshHighRateLimiting

## Summary
More than 10 requests per second are being rate limited for over 5 minutes.

## Severity
Warning

## Impact
Some users or clients are being denied service due to rate limiting.

## Investigation Steps

1. **Check rate limiting metrics**
   ```promql
   sum(rate(rate_limit_exceeded_total[5m]))
   ```

2. **Identify top rate-limited IPs/keys**
   ```bash
   kubectl logs -l app=shadowmesh-gateway --tail=1000 | grep "Rate limit exceeded" | \
     awk '{print $NF}' | sort | uniq -c | sort -rn | head -20
   ```

3. **Check Redis for rate limit data (if using distributed)**
   ```bash
   kubectl exec -it deploy/redis -- redis-cli keys "shadowmesh:ratelimit:*" | head -20
   ```

4. **Review request patterns**
   ```promql
   sum(rate(http_requests_total[5m])) by (endpoint)
   ```

5. **Check for potential attacks**
   - Look for unusual request patterns
   - Check for scraping behavior
   - Review User-Agent headers

## Resolution Steps

1. **If legitimate traffic spike:**
   - Temporarily increase rate limits:
     ```yaml
     # Update ConfigMap
     SHADOWMESH_RATE_LIMIT_REQUESTS_PER_SECOND: "200"
     ```
   - Consider horizontal scaling

2. **If suspected attack:**
   - Identify attacking IPs
   - Add IPs to block list (if available)
   - Consider enabling additional DDoS protection
   - Contact security team

3. **If misconfigured client:**
   - Identify the client (check API key if authenticated)
   - Contact the client to fix their request pattern
   - Consider providing higher limits for specific keys

4. **If rate limits too low:**
   - Review current limits vs actual traffic
   - Adjust limits based on capacity:
     ```bash
     kubectl edit configmap shadowmesh-config
     ```
   - Restart pods to apply changes

## Escalation
- If suspected DDoS attack, escalate to security team immediately
- If legitimate traffic causing issues, escalate to capacity planning

## Prevention
- Set rate limits based on capacity testing
- Implement tiered rate limiting for different client types
- Monitor rate limit metrics as part of regular operations
- Implement API key-based higher limits for trusted clients
