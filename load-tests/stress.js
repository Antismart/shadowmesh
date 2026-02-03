/**
 * ShadowMesh Gateway - Stress Test
 *
 * Tests system behavior under heavy load with increasing user counts.
 * Run with: k6 run stress.js --env BASE_URL=http://localhost:8081
 */

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Counter, Trend } from 'k6/metrics';

// Custom metrics
const errorRate = new Rate('errors');
const requestCount = new Counter('total_requests');
const latencyTrend = new Trend('latency_trend');

export const options = {
  stages: [
    { duration: '2m', target: 50 },     // Ramp to 50 users
    { duration: '3m', target: 50 },     // Stay at 50
    { duration: '2m', target: 100 },    // Ramp to 100
    { duration: '3m', target: 100 },    // Stay at 100
    { duration: '2m', target: 200 },    // Spike to 200
    { duration: '3m', target: 200 },    // Stay at 200
    { duration: '2m', target: 100 },    // Scale down to 100
    { duration: '2m', target: 50 },     // Scale down to 50
    { duration: '2m', target: 0 },      // Ramp down
  ],
  thresholds: {
    http_req_duration: ['p(95)<2000'],   // 95% < 2s under stress
    errors: ['rate<0.05'],                // Error rate < 5%
    http_req_failed: ['rate<0.05'],       // Failed requests < 5%
  },
};

const BASE_URL = __ENV.BASE_URL || 'http://localhost:8081';
const API_KEY = __ENV.API_KEY || '';

const headers = API_KEY ? { 'Authorization': `Bearer ${API_KEY}` } : {};

export default function() {
  const scenario = Math.random();

  if (scenario < 0.5) {
    // 50% - Health checks (simulates monitoring/load balancer probes)
    const res = http.get(`${BASE_URL}/health`);
    requestCount.add(1);
    latencyTrend.add(res.timings.duration);

    const ok = check(res, {
      'health status 200': (r) => r.status === 200,
    });
    errorRate.add(!ok);

  } else if (scenario < 0.8) {
    // 30% - Metrics scraping (simulates Prometheus)
    const res = http.get(`${BASE_URL}/metrics/prometheus`);
    requestCount.add(1);
    latencyTrend.add(res.timings.duration);

    const ok = check(res, {
      'prometheus metrics status 200': (r) => r.status === 200,
    });
    errorRate.add(!ok);

  } else if (scenario < 0.95) {
    // 15% - Dashboard access
    const res = http.get(`${BASE_URL}/dashboard`);
    requestCount.add(1);
    latencyTrend.add(res.timings.duration);

    const ok = check(res, {
      'dashboard status 200': (r) => r.status === 200,
    });
    errorRate.add(!ok);

  } else {
    // 5% - Index page
    const res = http.get(`${BASE_URL}/`);
    requestCount.add(1);
    latencyTrend.add(res.timings.duration);

    const ok = check(res, {
      'index status 200': (r) => r.status === 200,
    });
    errorRate.add(!ok);
  }

  // Short random sleep to simulate realistic traffic patterns
  sleep(Math.random() * 0.5 + 0.1);
}

export function handleSummary(data) {
  const { metrics } = data;

  console.log('\n========== STRESS TEST RESULTS ==========');
  console.log(`Total Requests: ${metrics.http_reqs.values.count}`);
  console.log(`Request Rate: ${metrics.http_reqs.values.rate.toFixed(2)}/s`);
  console.log(`Avg Duration: ${metrics.http_req_duration.values.avg.toFixed(2)}ms`);
  console.log(`P95 Duration: ${metrics.http_req_duration.values['p(95)'].toFixed(2)}ms`);
  console.log(`P99 Duration: ${metrics.http_req_duration.values['p(99)'].toFixed(2)}ms`);
  console.log(`Max Duration: ${metrics.http_req_duration.values.max.toFixed(2)}ms`);
  console.log(`Error Rate: ${(metrics.errors.values.rate * 100).toFixed(2)}%`);
  console.log('==========================================\n');

  return {
    'stress-results.json': JSON.stringify(data, null, 2),
  };
}
