/* Form field: label + control + one hint or one error line. Spacing is decided
   once here, so pages no longer each write their own mb-3 / mt-1. */
import type { ReactNode } from "react";
import { cn } from "./index";

export function Field({
  label,
  hint,
  error,
  htmlFor,
  className,
  children,
}: {
  label: ReactNode;
  /** One line of description below the control */
  hint?: ReactNode;
  /** Replaces the description with a danger-colored error when present */
  error?: ReactNode;
  htmlFor?: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div className={cn("mb-4", className)}>
      <label htmlFor={htmlFor} className="mb-1 block text-small text-ink-2">
        {label}
      </label>
      {children}
      {(error || hint) && (
        <p className={cn("mt-1 text-fine", error ? "text-danger" : "text-ink-3")}>
          {error ?? hint}
        </p>
      )}
    </div>
  );
}
