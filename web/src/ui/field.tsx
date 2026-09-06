/* Form field: label + control + a hint or an error line. Spacing is set once here,
   so pages don't each hand-roll their own mb-3 / mt-1. */
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
  /** A line of explanation below the control */
  hint?: ReactNode;
  /** Replaces the hint when there's an error, in the danger color */
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
