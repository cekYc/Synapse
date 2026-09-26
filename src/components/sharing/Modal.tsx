// ============================================================
// Synapse — Modal Dialog
// ============================================================

import { useEffect, type ReactNode } from 'react';
import { X } from 'lucide-react';

interface ModalProps {
  title: ReactNode;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
}

export function Modal({ title, onClose, children, footer }: ModalProps) {
  // Close on Escape
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <div className="modal-overlay" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="modal" role="dialog" aria-modal="true">
        <div className="modal__header">
          <div className="modal__title">{title}</div>
          <button className="btn btn-icon btn-ghost" onClick={onClose} aria-label="Kapat">
            <X size={16} />
          </button>
        </div>
        <div className="modal__body">{children}</div>
        {footer && <div className="modal__footer">{footer}</div>}
      </div>
    </div>
  );
}
