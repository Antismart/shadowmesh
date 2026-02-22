import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { deployments as deploymentsApi } from '../api/deployments';
import { github as githubApi } from '../api/github';
import type { GithubRepo } from '../api/types';
import { useAuth } from '../context/AuthContext';
import { useToast } from '../context/ToastContext';
import FileDropZone from '../components/FileDropZone';

export default function NewDeploymentPage() {
  const navigate = useNavigate();
  const { connected } = useAuth();
  const { addToast } = useToast();
  const [tab, setTab] = useState<'github' | 'upload'>('github');
  const [repos, setRepos] = useState<GithubRepo[]>([]);
  const [selectedRepo, setSelectedRepo] = useState('');
  const [branch, setBranch] = useState('');
  const [deploying, setDeploying] = useState(false);

  useEffect(() => {
    if (connected) {
      githubApi.repos().then(setRepos).catch(() => {});
    }
  }, [connected]);

  const handleGithubDeploy = async () => {
    if (!selectedRepo) return;
    const repo = repos.find((r) => r.full_name === selectedRepo);
    if (!repo) return;
    setDeploying(true);
    try {
      const result = await deploymentsApi.deployGithub(repo.html_url, branch || repo.default_branch);
      addToast('success', `Deployed ${repo.name}`);
      navigate(`/deployments/${result.cid}`);
    } catch (e) {
      addToast('error', `Deploy failed: ${e instanceof Error ? e.message : 'Unknown error'}`);
    } finally {
      setDeploying(false);
    }
  };

  const handleFileDeploy = async (file: File) => {
    setDeploying(true);
    try {
      const result = await deploymentsApi.deployZip(file);
      addToast('success', `Deployed ${file.name}`);
      navigate(`/deployments/${result.cid}`);
    } catch (e) {
      addToast('error', `Upload failed: ${e instanceof Error ? e.message : 'Unknown error'}`);
    } finally {
      setDeploying(false);
    }
  };

  const tabs = [
    { key: 'github' as const, label: 'Import from GitHub' },
    { key: 'upload' as const, label: 'Upload ZIP' },
  ];

  return (
    <div>
      <h1 className="text-2xl font-semibold text-mesh-text mb-2">Import Project</h1>
      <p className="text-sm text-mesh-muted mb-8">Deploy from a GitHub repository or upload a ZIP file.</p>

      {/* Tabs */}
      <div className="flex gap-6 border-b border-mesh-border mb-8">
        {tabs.map((t) => (
          <button
            key={t.key}
            onClick={() => setTab(t.key)}
            className={`pb-3 text-sm font-medium border-b-2 transition-colors ${
              tab === t.key
                ? 'text-mesh-text border-mesh-text'
                : 'text-mesh-muted border-transparent hover:text-mesh-text'
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>

      {tab === 'github' ? (
        <div className="border border-mesh-border rounded-lg p-6 max-w-xl">
          {!connected ? (
            <div className="text-center py-8">
              <p className="text-sm text-mesh-muted mb-4">Connect your GitHub account to deploy from repositories.</p>
              <a href="/api/github/login" className="btn-primary inline-flex items-center gap-2">
                <svg className="w-4 h-4" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z" />
                </svg>
                Connect GitHub
              </a>
            </div>
          ) : (
            <div className="space-y-4">
              <div>
                <label className="block text-sm font-medium text-mesh-text mb-2">Repository</label>
                <select
                  value={selectedRepo}
                  onChange={(e) => {
                    setSelectedRepo(e.target.value);
                    const repo = repos.find((r) => r.full_name === e.target.value);
                    setBranch(repo?.default_branch ?? '');
                  }}
                  className="input w-full"
                >
                  <option value="">Select a repository...</option>
                  {repos.map((repo) => (
                    <option key={repo.id} value={repo.full_name}>
                      {repo.full_name} {repo.private ? '(private)' : ''}
                    </option>
                  ))}
                </select>
              </div>

              {selectedRepo && (
                <div>
                  <label className="block text-sm font-medium text-mesh-text mb-2">Branch</label>
                  <input
                    type="text"
                    value={branch}
                    onChange={(e) => setBranch(e.target.value)}
                    placeholder="main"
                    className="input w-full"
                  />
                </div>
              )}

              <button
                onClick={handleGithubDeploy}
                disabled={!selectedRepo || deploying}
                className="btn-primary w-full"
              >
                {deploying ? 'Deploying...' : 'Deploy'}
              </button>
            </div>
          )}
        </div>
      ) : (
        <div className="max-w-xl">
          <FileDropZone onFile={handleFileDeploy} disabled={deploying} />
          {deploying && (
            <div className="mt-4 border border-mesh-border rounded-lg p-4">
              <div className="flex items-center gap-3">
                <div className="w-4 h-4 border-2 border-mesh-accent border-t-transparent rounded-full animate-spin" />
                <span className="text-sm text-mesh-muted">Deploying...</span>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
