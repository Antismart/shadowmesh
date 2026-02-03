/**
 * ShadowMesh Gateway - Smoke Test
 *
 * Basic functional test to verify the gateway is working correctly.
 * Run with: k6 run smoke.js --env BASE_URL=http://localhost:8081
 */

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Trend } from 'k6/metrics';

// Custom metrics
const errorRate = new Rate('errors');
const healthCheckDuration = new Trend('health_check_duration');

export const options = {
  stages: [
    { duration: '30s', target: 5 },    // Ramp up to 5 users
    { duration: '1m', target: 5 },     // Stay at 5 users
    { duration: '30s', target: 10 },   // Ramp up to 10 users
    { duration: '2m', target: 10 },    // Stay at 10 users
    { duration: '30s', target: 0 },    // Ramp down
  ],
  thresholds: {
    http_req_duration: ['p(95)<500'],   // 95% of requests < 500ms
    errors: ['rate<0.01'],               // Error rate < 1%
    health_check_duration: ['p(99)<100'], // Health checks < 100ms
  },
};

const BASE_URL = __ENV.BASE_URL || 'http://localhost:8081';
const API_KEY = __ENV.API_KEY || '';

const headers = API_KEY ? { 'Authorization': `Bearer ${API_KEY}` } : {};

export default function() {
  // Health check
  const healthStart = Date.now();
  const healthRes = http.get(`${BASE_URL}/health`);
  healthCheckDuration.add(Date.now() - healthStart);

  const healthOk = check(healthRes, {
    'health check status 200': (r) => r.status === 200,
    'health check has status field': (r) => {
      try {
        return JSON.parse(r.body).status !== undefined;
      } catch (e) {
        return false;
      }
    },
    'health check response time OK': (r) => r.timings.duration < 100,
  });
  errorRate.add(!healthOk);

  sleep(0.5);

  // Metrics endpoint
  const metricsRes = http.get(`${BASE_URL}/metrics`);
  const metricsOk = check(metricsRes, {
    'metrics status 200': (r) => r.status === 200,
    'metrics has uptime': (r) => r.body.includes('uptime'),
  });
  errorRate.add(!metricsOk);

  sleep(0.5);

  // Prometheus metrics endpoint
  const promRes = http.get(`${BASE_URL}/metrics/prometheus`);
  const promOk = check(promRes, {
    'prometheus metrics status 200': (r) => r.status === 200,
    'prometheus metrics has correct content type': (r) =>
      r.headers['Content-Type'].includes('text/plain'),
  });
  errorRate.add(!promOk);

  sleep(0.5);

  // Dashboard (HTML)
  const dashRes = http.get(`${BASE_URL}/dashboard`);
  const dashOk = check(dashRes, {
    'dashboard status 200': (r) => r.status === 200,
    'dashboard returns HTML': (r) => r.body.includes('<!DOCTYPE html>') || r.body.includes('<html'),
  });
  errorRate.add(!dashOk);

  sleep(0.5);
}

export function handleSummary(data) {
  return {
    'stdout': textSummary(data, { indent: '  ', enableColors: true }),
    'smoke-results.json': JSON.stringify(data, null, 2),
  };
}

function textSummary(data, options) {
  const { metrics } = data;
  const lines = [
    '\n========== SMOKE TEST RESULTS ==========\n',
    `Total Requests: ${metrics.http_reqs.values.count}`,
    `Request Rate: ${metrics.http_reqs.values.rate.toFixed(2)}/s`,
    `Avg Duration: ${metrics.http_req_duration.values.avg.toFixed(2)}ms`,
    `P95 Duration: ${metrics.http_req_duration.values['p(95)'].toFixed(2)}ms`,
    `Error Rate: ${(metrics.errors.values.rate * 100).toFixed(2)}%`,
    '\n=========================================\n',
  ];
  return lines.join('\n');
}
