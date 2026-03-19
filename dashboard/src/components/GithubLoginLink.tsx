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
 *
 * Uses an absolute URL to avoid being affected by the <base> tag when
 * the dashboard is served under a CID path.
 */
export default function GithubLoginLink({ className, children }: Props) {
  const [href, setHref] = useState('#');

  useEffect(() => {
    const currentUrl = window.location.href.split('?')[0];
    gatewayUrl(`/api/github/login?redirect_to=${encodeURIComponent(currentUrl)}`).then((url) => {
      // Ensure it's an absolute URL so the <base> tag doesn't interfere
      if (url.startsWith('/')) {
        setHref(window.location.origin + url);
      } else {
        setHref(url);
      }
    });
  }, []);

  return (
    <a href={href} className={className}>
      {children}
    </a>
  );
}
