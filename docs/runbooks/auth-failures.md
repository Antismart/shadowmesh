# Alert: ShadowMeshAuthFailureSpike

## Summary
More than 5 authentication failures per second for over 5 minutes.

## Severity
Warning

## Impact
- Possible brute force attack in progress
- Legitimate users may have expired/invalid API keys
- Service may be under credential stuffing attack

## Investigation Steps

1. **Check auth failure metrics**
   ```promql
   sum(rate(auth_failures_total[5m])) by (reason)
   ```

2. **Review auth failure logs**
   ```bash
   kubectl logs -l app=shadowmesh-gateway --tail=1000 | grep -i "auth" | grep -i "fail"
   ```

3. **Identify source IPs**
   ```bash
   kubectl logs -l app=shadowmesh-gateway --tail=1000 | grep "AuthFailure" | \
     grep -oE '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+' | sort | uniq -c | sort -rn | head -20
   ```

4. **Check audit logs**
   ```bash
   kubectl exec -it deploy/shadowmesh-gateway -- curl -s http://localhost:8081/api/audit/recent | jq '.[] | select(.action == "AuthFailure")'
   ```

5. **Verify API key validity**
   - Check if keys are expired
   - Verify keys haven't been rotated
   - Ensure environment variables are correct

## Resolution Steps

1. **If brute force attack:**
   - Block attacking IPs at network level (WAF, firewall)
   - Enable additional rate limiting on auth endpoints
   - Consider implementing CAPTCHA or additional verification
   - Alert security team

2. **If expired API keys:**
   - Identify affected clients
   - Rotate keys if necessary:
     ```bash
     # Generate new key
     openssl rand -hex 32
     ```
   - Update Kubernetes secret
   - Notify affected clients

3. **If configuration issue:**
   - Verify API keys in secret:
     ```bash
     kubectl get secret shadowmesh-secrets -o jsonpath='{.data.SHADOWMESH_API_KEYS}' | base64 -d
     ```
   - Check for whitespace or encoding issues
   - Restart pods after fixing

4. **If client misconfiguration:**
   - Work with client to verify their API key
   - Check if key is being sent correctly (Bearer token)
   - Verify client isn't hitting wrong endpoint

## Escalation
- Suspected credential attack: Escalate to security team immediately
- Widespread key issues: Escalate to operations team

## Prevention
- Implement API key expiration and rotation
- Monitor auth failure trends
- Use strong, long API keys (256+ bits)
- Implement account lockout after repeated failures
- Set up alerting for unusual auth patterns
