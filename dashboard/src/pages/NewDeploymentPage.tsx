import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { deployments as deploymentsApi } from '../api/deployments';
import { github as githubApi } from '../api/github';
import { gatewayUrl } from '../api/client';
import type { GithubRepo, DeployResponse } from '../api/types';
import { useAuth } from '../context/AuthContext';
import { useToast } from '../context/ToastContext';
import FileDropZone from '../components/FileDropZone';
import FolderBrowser from '../components/FolderBrowser';
import BuildLogViewer from '../components/BuildLogViewer';
import GithubLoginLink from '../components/GithubLoginLink';

type BuildStatus = 'building' | 'success' | 'error' | null;

interface EnvVar {
  key: string;
  value: string;
}

export default function NewDeploymentPage() {
  const navigate = useNavigate();
  const { connected } = useAuth();
  const { addToast } = useToast();
  const [tab, setTab] = useState<'github' | 'upload'>('github');
  const [repos, setRepos] = useState<GithubRepo[]>([]);
  const [selectedRepo, setSelectedRepo] = useState('');
  const [branch, setBranch] = useState('');
  const [rootDirectory, setRootDirectory] = useState('');
  const [deploying, setDeploying] = useState(false);

  // Build settings state
  const [showBuildSettings, setShowBuildSettings] = useState(false);
  const [envVars, setEnvVars] = useState<EnvVar[]>([]);
  const [buildCommand, setBuildCommand] = useState('');
  const [outputDirectory, setOutputDirectory] = useState('');

  // SSE streaming state
  const [buildLogs, setBuildLogs] = useState<string[]>([]);
  const [buildStatus, setBuildStatus] = useState<BuildStatus>(null);
  const [buildResult, setBuildResult] = useState<DeployResponse | null>(null);
  const [buildError, setBuildError] = useState<string | null>(null);
  const eventSourceRef = useRef<EventSource | null>(null);

  useEffect(() => {
    if (connected) {
      githubApi.repos().then(setRepos).catch(() => {});
    }
  }, [connected]);

  // Cleanup EventSource on unmount
  useEffect(() => {
    return () => {
      eventSourceRef.current?.close();
    };
  }, []);

  const resetBuild = () => {
    setBuildLogs([]);
    setBuildStatus(null);
    setBuildResult(null);
    setBuildError(null);
    eventSourceRef.current?.close();
    eventSourceRef.current = null;
  };

  const handleGithubDeploy = async () => {
    if (!selectedRepo) return;
    const repo = repos.find((r) => r.full_name === selectedRepo);
    if (!repo) return;

    resetBuild();
    setDeploying(true);
    setBuildStatus('building');

    try {
      // Build env vars map from the key-value pairs
      const envVarsMap: Record<string, string> = {};
      for (const ev of envVars) {
        const k = ev.key.trim();
        if (k) {
          envVarsMap[k] = ev.value;
        }
      }

      const { deploy_id, stream_url } = await deploymentsApi.deployGithubAsync({
        url: repo.html_url,
        branch: branch || repo.default_branch,
        rootDirectory: rootDirectory || undefined,
        envVars: Object.keys(envVarsMap).length > 0 ? envVarsMap : undefined,
        buildCommand: buildCommand.trim() || undefined,
        outputDirectory: outputDirectory.trim() || undefined,
      });

      const fullStreamUrl = await gatewayUrl(stream_url);
      const es = new EventSource(fullStreamUrl);
      eventSourceRef.current = es;

      es.addEventListener('log', (e) => {
        setBuildLogs((prev) => [...prev, e.data]);
      });

      es.addEventListener('complete', (e) => {
        try {
          const result = JSON.parse(e.data);
          setBuildResult(result);
          setBuildStatus('success');
        } catch {
          setBuildStatus('success');
        }
        setDeploying(false);
        es.close();
      });

      es.addEventListener('error', (e) => {
        if (e instanceof MessageEvent && e.data) {
          setBuildError(e.data);
          setBuildLogs((prev) => [...prev, `Error: ${e.data}`]);
        } else {
          setBuildError('Build failed');
        }
        setBuildStatus('error');
        setDeploying(false);
        es.close();
      });

      // Handle connection errors
      es.onerror = () => {
        if (buildStatus === 'building') {
          setBuildError('Connection to build stream lost');
          setBuildStatus('error');
          setDeploying(false);
        }
        es.close();
      };
    } catch (e) {
      setBuildError(e instanceof Error ? e.message : 'Deploy failed');
      setBuildStatus('error');
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

  // Show build view when deploying or when we have a result
  const showBuildView = buildStatus !== null;

  return (
    <div>
      <h1 className="text-2xl font-semibold text-mesh-text mb-2">Import Project</h1>
      <p className="text-sm text-mesh-muted mb-8">Deploy from a GitHub repository or upload a ZIP file.</p>

      {/* Success Banner */}
      {buildStatus === 'success' && buildResult && (
        <div className="mb-6 border border-mesh-accent/30 rounded-lg bg-mesh-accent/5 p-4">
          <div className="flex items-start gap-3">
            <svg className="w-5 h-5 text-mesh-accent flex-shrink-0 mt-0.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <div className="flex-1 min-w-0">
              <p className="text-sm font-medium text-mesh-accent">Deployment Successful</p>
              <p className="text-xs text-mesh-muted mt-1 font-mono truncate">CID: {buildResult.cid}</p>
              {buildResult.shadow_url && (
                <a
                  href={buildResult.shadow_url}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-xs text-mesh-accent hover:underline mt-1 inline-block"
                >
                  {buildResult.shadow_url}
                </a>
              )}
              <div className="flex gap-3 mt-3">
                <button
                  onClick={() => navigate(`/deployments/${buildResult.cid}`)}
                  className="btn-primary text-xs px-3 py-1.5"
                >
                  View Deployment
                </button>
                {buildResult.url && (
                  <a
                    href={buildResult.url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-xs px-3 py-1.5 border border-mesh-border rounded text-mesh-text hover:bg-mesh-surface transition-colors inline-flex items-center gap-1"
                  >
                    Visit Site
                    <svg className="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                      <path strokeLinecap="round" strokeLinejoin="round" d="M13.5 6H5.25A2.25 2.25 0 003 8.25v10.5A2.25 2.25 0 005.25 21h10.5A2.25 2.25 0 0018 18.75V10.5m-10.5 6L21 3m0 0h-5.25M21 3v5.25" />
                    </svg>
                  </a>
                )}
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Error Banner */}
      {buildStatus === 'error' && (
        <div className="mb-6 border border-[#ee0000]/30 rounded-lg bg-[#ee0000]/5 p-4">
          <div className="flex items-start gap-3">
            <svg className="w-5 h-5 text-[#ee0000] flex-shrink-0 mt-0.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M9.75 9.75l4.5 4.5m0-4.5l-4.5 4.5M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <div className="flex-1 min-w-0">
              <p className="text-sm font-medium text-[#ee0000]">Deployment Failed</p>
              <p className="text-xs text-mesh-muted mt-1">{buildError || 'An unknown error occurred'}</p>
              <button
                onClick={() => {
                  resetBuild();
                  setDeploying(false);
                }}
                className="mt-3 text-xs px-3 py-1.5 border border-mesh-border rounded text-mesh-text hover:bg-mesh-surface transition-colors"
              >
                Try Again
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Build Logs (shown during/after build) */}
      {showBuildView && (
        <div className="mb-6">
          <BuildLogViewer
            logs=""
            status={buildStatus === 'success' ? 'Built' : buildStatus === 'error' ? 'Failed' : 'Building'}
            streamingLines={buildLogs}
            isStreaming={buildStatus === 'building'}
          />
        </div>
      )}

      {/* Tabs - hide form during active build */}
      {!deploying && buildStatus !== 'building' && (
        <>
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
                  <GithubLoginLink className="btn-primary inline-flex items-center gap-2">
                    <svg className="w-4 h-4" viewBox="0 0 24 24" fill="currentColor">
                      <path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z" />
                    </svg>
                    Connect GitHub
                  </GithubLoginLink>
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
                    <>
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
                      <div>
                        <label className="block text-sm font-medium text-mesh-text mb-2">Root Directory</label>
                        <FolderBrowser
                          repo={selectedRepo}
                          branch={branch || repos.find((r) => r.full_name === selectedRepo)?.default_branch || 'main'}
                          value={rootDirectory}
                          onChange={setRootDirectory}
                        />
                        <p className="text-xs text-mesh-muted mt-1">
                          Select the directory containing your project.
                        </p>
                      </div>

                      {/* Build Settings (collapsible) */}
                      <div className="border border-mesh-border rounded-lg">
                        <button
                          type="button"
                          onClick={() => setShowBuildSettings(!showBuildSettings)}
                          className="w-full flex items-center justify-between px-4 py-3 text-sm font-medium text-mesh-text hover:bg-mesh-surface transition-colors rounded-lg"
                        >
                          <span>Build Settings</span>
                          <svg
                            className={`w-4 h-4 text-mesh-muted transition-transform ${showBuildSettings ? 'rotate-180' : ''}`}
                            fill="none"
                            viewBox="0 0 24 24"
                            stroke="currentColor"
                            strokeWidth={2}
                          >
                            <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 8.25l-7.5 7.5-7.5-7.5" />
                          </svg>
                        </button>

                        {showBuildSettings && (
                          <div className="px-4 pb-4 space-y-4 border-t border-mesh-border">
                            {/* Build Command */}
                            <div className="pt-4">
                              <label className="block text-sm font-medium text-mesh-text mb-2">Build Command</label>
                              <input
                                type="text"
                                value={buildCommand}
                                onChange={(e) => setBuildCommand(e.target.value)}
                                placeholder="auto-detected (e.g., npm run build)"
                                className="input w-full"
                              />
                              <p className="text-xs text-mesh-muted mt-1">
                                Override the auto-detected build command.
                              </p>
                            </div>

                            {/* Output Directory */}
                            <div>
                              <label className="block text-sm font-medium text-mesh-text mb-2">Output Directory</label>
                              <input
                                type="text"
                                value={outputDirectory}
                                onChange={(e) => setOutputDirectory(e.target.value)}
                                placeholder="auto-detected (e.g., dist)"
                                className="input w-full"
                              />
                              <p className="text-xs text-mesh-muted mt-1">
                                Override the auto-detected output directory.
                              </p>
                            </div>

                            {/* Environment Variables */}
                            <div>
                              <label className="block text-sm font-medium text-mesh-text mb-2">Environment Variables</label>
                              {envVars.length > 0 && (
                                <div className="space-y-2 mb-2">
                                  {envVars.map((ev, idx) => (
                                    <div key={idx} className="flex items-center gap-2">
                                      <input
                                        type="text"
                                        value={ev.key}
                                        onChange={(e) => {
                                          const updated = [...envVars];
                                          updated[idx] = { ...updated[idx], key: e.target.value };
                                          setEnvVars(updated);
                                        }}
                                        placeholder="NAME"
                                        className="input flex-1 font-mono text-xs"
                                      />
                                      <input
                                        type="text"
                                        value={ev.value}
                                        onChange={(e) => {
                                          const updated = [...envVars];
                                          updated[idx] = { ...updated[idx], value: e.target.value };
                                          setEnvVars(updated);
                                        }}
                                        placeholder="value"
                                        className="input flex-1 font-mono text-xs"
                                      />
                                      <button
                                        type="button"
                                        onClick={() => {
                                          setEnvVars(envVars.filter((_, i) => i !== idx));
                                        }}
                                        className="btn-ghost px-2 py-1.5 text-mesh-muted hover:text-[#ee0000] transition-colors"
                                        title="Remove variable"
                                      >
                                        <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                                          <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                                        </svg>
                                      </button>
                                    </div>
                                  ))}
                                </div>
                              )}
                              <button
                                type="button"
                                onClick={() => setEnvVars([...envVars, { key: '', value: '' }])}
                                className="btn-ghost text-xs inline-flex items-center gap-1"
                              >
                                <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                                  <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
                                </svg>
                                Add Variable
                              </button>
                              <p className="text-xs text-mesh-muted mt-1">
                                These variables will be available during the build process.
                              </p>
                            </div>
                          </div>
                        )}
                      </div>
                    </>
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
        </>
      )}

      {/* Loading state during build - show when form is hidden */}
      {deploying && buildStatus === 'building' && buildLogs.length === 0 && (
        <div className="border border-mesh-border rounded-lg p-6 max-w-xl">
          <div className="flex items-center gap-3">
            <div className="w-4 h-4 border-2 border-mesh-accent border-t-transparent rounded-full animate-spin" />
            <span className="text-sm text-mesh-muted">Starting build...</span>
          </div>
        </div>
      )}
    </div>
  );
}
