import React, { Suspense } from 'react';
import { Routes, Route } from 'react-router-dom';
import RootLayout from './layout/RootLayout';
import LoginPage from './pages/LoginPage';
import LoadingSkeleton from './components/LoadingSkeleton';

const OverviewPage = React.lazy(() => import('./pages/OverviewPage'));
const ProjectDetailPage = React.lazy(() => import('./pages/ProjectDetailPage'));
const DeploymentDetailPage = React.lazy(() => import('./pages/DeploymentDetailPage'));
const NewDeploymentPage = React.lazy(() => import('./pages/NewDeploymentPage'));
const DomainsPage = React.lazy(() => import('./pages/DomainsPage'));
const AnalyticsPage = React.lazy(() => import('./pages/AnalyticsPage'));
const SettingsPage = React.lazy(() => import('./pages/SettingsPage'));
const NotFoundPage = React.lazy(() => import('./pages/NotFoundPage'));

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route path="/" element={<RootLayout />}>
        <Route
          index
          element={
            <Suspense fallback={<LoadingSkeleton />}>
              <OverviewPage />
            </Suspense>
          }
        />
        <Route
          path="projects/:repoName"
          element={
            <Suspense fallback={<LoadingSkeleton />}>
              <ProjectDetailPage />
            </Suspense>
          }
        />
        <Route
          path="deployments/:cid"
          element={
            <Suspense fallback={<LoadingSkeleton />}>
              <DeploymentDetailPage />
            </Suspense>
          }
        />
        <Route
          path="new"
          element={
            <Suspense fallback={<LoadingSkeleton />}>
              <NewDeploymentPage />
            </Suspense>
          }
        />
        <Route
          path="domains"
          element={
            <Suspense fallback={<LoadingSkeleton />}>
              <DomainsPage />
            </Suspense>
          }
        />
        <Route
          path="analytics"
          element={
            <Suspense fallback={<LoadingSkeleton />}>
              <AnalyticsPage />
            </Suspense>
          }
        />
        <Route
          path="settings"
          element={
            <Suspense fallback={<LoadingSkeleton />}>
              <SettingsPage />
            </Suspense>
          }
        />
        <Route
          path="*"
          element={
            <Suspense fallback={<LoadingSkeleton />}>
              <NotFoundPage />
            </Suspense>
          }
        />
      </Route>
    </Routes>
  );
}
