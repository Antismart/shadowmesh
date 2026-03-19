import { useEffect, useState, type ReactNode } from 'react';
import { gatewayUrl } from '../api/client';

interface Props {
  className?: string;
  children: ReactNode;
}

/**
 * An <a> that points to the active gateway's GitHub OAuth login endpoint.
 * Resolves the gateway URL on mount so the link works when the dashboard
 * is deployed as static content outside of a gateway origin.
 */
export default function GithubLoginLink({ className, children }: Props) {
  const [href, setHref] = useState('/api/github/login');

  useEffect(() => {
    gatewayUrl('/api/github/login').then(setHref);
  }, []);

  return (
    <a href={href} className={className}>
      {children}
    </a>
  );
}
