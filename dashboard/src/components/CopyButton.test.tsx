import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import CopyButton from './CopyButton';

// Mock the clipboard API
Object.assign(navigator, {
  clipboard: {
    writeText: vi.fn().mockResolvedValue(undefined),
  },
});

describe('CopyButton', () => {
  it('renders with default "Copy" label', () => {
    render(<CopyButton text="hello" />);
    expect(screen.getByText('Copy')).toBeInTheDocument();
  });

  it('renders with a custom label', () => {
    render(<CopyButton text="hello" label="Copy CID" />);
    expect(screen.getByText('Copy CID')).toBeInTheDocument();
  });

  it('renders as a button element', () => {
    render(<CopyButton text="hello" />);
    const button = screen.getByRole('button');
    expect(button).toBeInTheDocument();
  });

  it('copies text to clipboard on click and shows "Copied"', async () => {
    render(<CopyButton text="some-cid-hash" />);

    const button = screen.getByRole('button');
    fireEvent.click(button);

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('some-cid-hash');

    await waitFor(() => {
      expect(screen.getByText('Copied')).toBeInTheDocument();
    });
  });
});
