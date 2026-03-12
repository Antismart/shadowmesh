import { apiFetch } from './client';
import type { GithubStatus, GithubRepo } from './types';

export interface TreeEntry {
  path: string;
  type: 'tree' | 'blob';
}

export const github = {
  status: () => apiFetch<GithubStatus>('/api/github/status'),

  repos: () => apiFetch<GithubRepo[]>('/api/github/repos'),

  tree: (repo: string, branch: string) =>
    apiFetch<{ entries: TreeEntry[] }>(
      `/api/github/tree?repo=${encodeURIComponent(repo)}&branch=${encodeURIComponent(branch)}`,
    ),

  loginUrl: () => '/api/github/login',
};
