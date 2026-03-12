import Modal from './Modal';

interface ConfirmDialogProps {
  open: boolean;
  onClose: () => void;
  onConfirm: () => void;
  title: string;
  message: string;
  confirmLabel?: string;
  loadingLabel?: string;
  confirmVariant?: 'danger' | 'primary';
  loading?: boolean;
}

export default function ConfirmDialog({
  open,
  onClose,
  onConfirm,
  title,
  message,
  confirmLabel = 'Delete',
  loadingLabel,
  confirmVariant = 'danger',
  loading = false,
}: ConfirmDialogProps) {
  const btnClass = confirmVariant === 'primary' ? 'btn-primary' : 'btn-danger';
  return (
    <Modal open={open} onClose={onClose} title={title}>
      <p className="text-sm text-mesh-muted mb-6">{message}</p>
      <div className="flex justify-end gap-3">
        <button onClick={onClose} className="btn-secondary" disabled={loading}>
          Cancel
        </button>
        <button onClick={onConfirm} className={btnClass} disabled={loading}>
          {loading ? (loadingLabel || 'Processing...') : confirmLabel}
        </button>
      </div>
    </Modal>
  );
}
