import { apiFetch, apiPost, apiDelete, apiUpload } from './client';
import type { Deployment, DeployResponse, DeployLogsResponse, AsyncDeployResponse } from './types';

export interface DeployGithubOptions {
  url: string;
  branch: string;
  rootDirectory?: string;
  envVars?: Record<string, string>;
  buildCommand?: string;
  outputDirectory?: string;
}

export const deployments = {
  list: () => apiFetch<Deployment[]>('/api/deployments'),

  deployZip: (file: File) => {
    const fd = new FormData();
    fd.append('file', file);
    return apiUpload<DeployResponse>('/api/deploy', fd);
  },

  deployGithub: (url: string, branch: string, rootDirectory?: string) =>
    apiPost<DeployResponse>('/api/deploy/github', {
      url,
      branch,
      ...(rootDirectory ? { root_directory: rootDirectory } : {}),
    }),

  deployGithubAsync: (opts: DeployGithubOptions) =>
    apiPost<AsyncDeployResponse>('/api/deploy/github', {
      url: opts.url,
      branch: opts.branch,
      ...(opts.rootDirectory ? { root_directory: opts.rootDirectory } : {}),
      ...(opts.envVars && Object.keys(opts.envVars).length > 0
        ? { env_vars: opts.envVars }
        : {}),
      ...(opts.buildCommand ? { build_command: opts.buildCommand } : {}),
      ...(opts.outputDirectory ? { output_directory: opts.outputDirectory } : {}),
    }),

  remove: (cid: string) =>
    apiDelete<{ success: boolean }>(`/api/deployments/${cid}`),

  redeploy: (cid: string) =>
    apiPost<DeployResponse>(`/api/deployments/${cid}/redeploy`, {}),

  logs: (cid: string) =>
    apiFetch<DeployLogsResponse>(`/api/deployments/${cid}/logs`),

  analytics: (cid: string) =>
    apiFetch<{ requests: number; bytes_served: number }>(`/api/deployments/${cid}/analytics`),
};
