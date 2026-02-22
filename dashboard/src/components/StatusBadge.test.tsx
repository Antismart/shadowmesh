import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import StatusBadge from './StatusBadge';

describe('StatusBadge', () => {
  it('renders the status text', () => {
    render(<StatusBadge status="Ready" />);
    expect(screen.getByText('Ready')).toBeInTheDocument();
  });

  it('renders "Failed" status text', () => {
    render(<StatusBadge status="Failed" />);
    expect(screen.getByText('Failed')).toBeInTheDocument();
  });

  it('renders "Building" status text', () => {
    render(<StatusBadge status="Building" />);
    expect(screen.getByText('Building')).toBeInTheDocument();
  });

  it('renders an unknown status with fallback dot color', () => {
    const { container } = render(<StatusBadge status="SomethingNew" />);
    expect(screen.getByText('SomethingNew')).toBeInTheDocument();
    // The fallback class is the Skipped color: bg-mesh-muted
    const dot = container.querySelector('span > span');
    expect(dot?.className).toContain('bg-mesh-muted');
  });

  it('applies the correct dot color for Ready status', () => {
    const { container } = render(<StatusBadge status="Ready" />);
    const dot = container.querySelector('span > span');
    expect(dot?.className).toContain('bg-[#0070f3]');
  });

  it('applies the correct dot color for Failed status', () => {
    const { container } = render(<StatusBadge status="Failed" />);
    const dot = container.querySelector('span > span');
    expect(dot?.className).toContain('bg-[#ee0000]');
  });

  it('applies the correct dot color for Building status', () => {
    const { container } = render(<StatusBadge status="Building" />);
    const dot = container.querySelector('span > span');
    expect(dot?.className).toContain('bg-[#f5a623]');
  });
});
