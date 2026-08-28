import { ReactNode, useEffect } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "motion/react";
import "../styles/modal.css";

export type ModalSize = "sm" | "md" | "lg" | "xl" | "full";

export interface ModalProps {
  isOpen?: boolean;
  onClose: () => void;
  title: ReactNode;
  subtitle?: ReactNode;
  icon?: ReactNode;
  actions?: ReactNode;
  tabs?: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  size?: ModalSize;
  className?: string;
  closeOnBackdrop?: boolean;
  closeOnEsc?: boolean;
}

export function Modal({
  isOpen = true,
  onClose,
  title,
  subtitle,
  icon,
  actions,
  tabs,
  children,
  footer,
  size = "lg",
  className = "",
  closeOnBackdrop = true,
  closeOnEsc = true,
}: ModalProps) {
  const { t } = useTranslation();

  useEffect(() => {
    if (!isOpen) return;

    const originalOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";

    const handleKeyDown = (e: KeyboardEvent) => {
      if (closeOnEsc && e.key === "Escape") {
        onClose();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => {
      document.body.style.overflow = originalOverflow;
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [isOpen, onClose, closeOnEsc]);

  return createPortal(
    <AnimatePresence>
      {isOpen ? (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.18 }}
          className="sonde-portal-backdrop"
          onClick={closeOnBackdrop ? onClose : undefined}
          role="dialog"
          aria-modal="true"
        >
          <motion.div
            initial={{ opacity: 0, scale: 0.96, y: 10 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.96, y: 10 }}
            transition={{ type: "spring", stiffness: 420, damping: 32 }}
            className={`sonde-portal-modal size-${size} ${className}`}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="sonde-modal-header">
              <div className="sonde-modal-title-group">
                {icon ? <div className="sonde-modal-icon">{icon}</div> : null}
                <div className="sonde-modal-title-text">
                  {typeof title === "string" ? <h2>{title}</h2> : title}
                  {subtitle ? <div className="sonde-modal-subtitle">{subtitle}</div> : null}
                </div>
              </div>

              <div className="sonde-modal-header-actions">
                {actions}
                <button
                  type="button"
                  className="sonde-modal-close-btn"
                  onClick={onClose}
                  aria-label={t("common.close")}
                >
                  <X size={18} />
                </button>
              </div>
            </div>

            {tabs ? <div className="sonde-modal-tabs">{tabs}</div> : null}

            <div className="sonde-modal-body">{children}</div>

            {footer ? <div className="sonde-modal-footer">{footer}</div> : null}
          </motion.div>
        </motion.div>
      ) : null}
    </AnimatePresence>,
    document.body
  );
}
