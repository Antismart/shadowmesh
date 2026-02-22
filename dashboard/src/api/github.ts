import { apiFetch } from './client';
import type { GithubStatus, GithubRepo } from './types';

export const github = {
  status: () => apiFetch<GithubStatus>('/api/github/status'),

  repos: () => apiFetch<GithubRepo[]>('/api/github/repos'),

  loginUrl: () => '/api/github/login',
};
