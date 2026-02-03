# Alert: ShadowMeshNoRequests

## Summary
The gateway has not received any requests in the last 5 minutes.

## Severity
Warning

## Impact
- Service may be unreachable
- Users cannot access the gateway
- Monitoring may indicate complete outage

## Investigation Steps

1. **Verify the alert is not a false positive**
   ```bash
   kubectl exec -it deploy/shadowmesh-gateway -- curl -s http://localhost:8081/health
   ```

2. **Check pod status**
   ```bash
   kubectl get pods -l app=shadowmesh-gateway
   kubectl describe pods -l app=shadowmesh-gateway
   ```

3. **Check service and endpoints**
   ```bash
   kubectl get svc shadowmesh-gateway
   kubectl get endpoints shadowmesh-gateway
   ```

4. **Check ingress configuration**
   ```bash
   kubectl get ingress
   kubectl describe ingress shadowmesh-gateway
   ```

5. **Test external connectivity**
   ```bash
   curl -v https://gateway.shadowmesh.io/health
   ```

6. **Check DNS resolution**
   ```bash
   nslookup gateway.shadowmesh.io
   dig gateway.shadowmesh.io
   ```

7. **Check load balancer status**
   - Verify cloud load balancer is healthy
   - Check for any cloud provider issues

## Resolution Steps

1. **If pods are not running:**
   ```bash
   kubectl rollout restart deployment/shadowmesh-gateway
   kubectl rollout status deployment/shadowmesh-gateway
   ```

2. **If service endpoints are empty:**
   - Check pod labels match service selector
   - Verify pods are passing readiness checks
   - Check for NetworkPolicy blocking traffic

3. **If ingress misconfigured:**
   - Verify ingress rules are correct
   - Check TLS certificate validity
   - Verify ingress controller is healthy:
     ```bash
     kubectl get pods -n ingress-nginx
     ```

4. **If DNS issue:**
   - Check DNS records
   - Verify DNS propagation
   - Check for DNS provider issues

5. **If load balancer issue:**
   - Check cloud provider status page
   - Verify load balancer health checks
   - Check security groups/firewall rules

6. **If network issue:**
   - Check NetworkPolicies
   - Verify cluster network health
   - Check for node network issues

## Escalation
Escalate immediately as this indicates potential complete outage.

## Prevention
- Set up synthetic monitoring (external health checks)
- Implement multi-region deployment
- Use proper readiness probes
- Monitor ingress controller health
- Set up DNS failover
