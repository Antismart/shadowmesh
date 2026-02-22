import { Routes, Route } from 'react-router-dom';
import RootLayout from './layout/RootLayout';
import LoginPage from './pages/LoginPage';
import OverviewPage from './pages/OverviewPage';
import ProjectDetailPage from './pages/ProjectDetailPage';
import DeploymentDetailPage from './pages/DeploymentDetailPage';
import NewDeploymentPage from './pages/NewDeploymentPage';
import DomainsPage from './pages/DomainsPage';
import AnalyticsPage from './pages/AnalyticsPage';
import SettingsPage from './pages/SettingsPage';
import NotFoundPage from './pages/NotFoundPage';

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route path="/" element={<RootLayout />}>
        <Route index element={<OverviewPage />} />
        <Route path="projects/:repoName" element={<ProjectDetailPage />} />
        <Route path="deployments/:cid" element={<DeploymentDetailPage />} />
        <Route path="new" element={<NewDeploymentPage />} />
        <Route path="domains" element={<DomainsPage />} />
        <Route path="analytics" element={<AnalyticsPage />} />
        <Route path="settings" element={<SettingsPage />} />
        <Route path="*" element={<NotFoundPage />} />
      </Route>
    </Routes>
  );
}
