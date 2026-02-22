import { apiFetch, apiPost, apiDelete, apiUpload } from './client';
import type { Deployment, DeployResponse, DeployLogsResponse } from './types';

export const deployments = {
  list: () => apiFetch<Deployment[]>('/api/deployments'),

  deployZip: (file: File) => {
    const fd = new FormData();
    fd.append('file', file);
    return apiUpload<DeployResponse>('/api/deploy', fd);
  },

  deployGithub: (url: string, branch: string) =>
    apiPost<DeployResponse>('/api/deploy/github', { url, branch }),

  remove: (cid: string) =>
    apiDelete<{ success: boolean }>(`/api/deployments/${cid}`),

  redeploy: (cid: string) =>
    apiPost<DeployResponse>(`/api/deployments/${cid}/redeploy`, {}),

  logs: (cid: string) =>
    apiFetch<DeployLogsResponse>(`/api/deployments/${cid}/logs`),
};
