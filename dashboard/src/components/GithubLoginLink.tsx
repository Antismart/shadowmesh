import { useEffect, useState, type ReactNode } from 'react';
import { gatewayUrl } from '../api/client';

interface Props {
  className?: string;
  children: ReactNode;
}

/**
 * An <a> that points to the active gateway's GitHub OAuth login endpoint.
 * Passes `redirect_to` so the auth gateway redirects back to the current
 * dashboard URL with a JWT token after OAuth completes.
 */
export default function GithubLoginLink({ className, children }: Props) {
  const [href, setHref] = useState('#');

  useEffect(() => {
    const currentUrl = window.location.origin + window.location.pathname;
    gatewayUrl(`/api/github/login?redirect_to=${encodeURIComponent(currentUrl)}`).then(setHref);
  }, []);

  return (
    <a href={href} className={className}>
      {children}
    </a>
  );
}
