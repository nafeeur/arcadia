/* Dialog: Radix Dialog underneath (focus trap, Esc, overlay click, all the
   aria wiring). Skin is styles.css's u-modal-*. Near-opaque rather than glass:
   a confirmation dialog needs someone to read one sentence and make an
   irreversible decision, and content showing through from below is a
   distraction (see the note on .u-modal-panel). */
import { Dialog as RadixDialog } from "radix-ui";
import { X } from "lucide-react";
import { useState } from "react";
import type { ReactNode } from "react";
import { Button, IconButton, Input, cn } from "./index";

export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  closeLabel,
  width = "md",
  children,
  footer,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  /** One line of description under the title; also the description a screen reader announces */
  description?: ReactNode;
  /** Accessible name for the top-right close button — the page pulls it from S, this component doesn't know the language */
  closeLabel: string;
  width?: "sm" | "md" | "lg";
  children?: ReactNode;
  /** Bottom-right button area. Use DangerConfirm for destructive confirmations, not assembled here */
  footer?: ReactNode;
}) {
  const w = { sm: "w-96", md: "w-[32rem]", lg: "w-[44rem]" }[width];
  return (
    <RadixDialog.Root open={open} onOpenChange={onOpenChange}>
      <RadixDialog.Portal>
        <RadixDialog.Overlay className="u-modal-scrim fixed inset-0 z-50 grid place-items-center overflow-y-auto p-4">
          <RadixDialog.Content
            className={cn(
              "u-modal-panel u-modal-in max-w-full rounded-xl p-6 shadow-2xl outline-none",
              w,
            )}
          >
            <div className="flex items-start gap-3">
              <div className="min-w-0 flex-1">
                <RadixDialog.Title className="u-title text-title">
                  {title}
                </RadixDialog.Title>
                {description ? (
                  <RadixDialog.Description className="mt-1 text-small text-ink-2">
                    {description}
                  </RadixDialog.Description>
                ) : (
                  // Radix warns in the console without a description; an empty one still counts
                  <RadixDialog.Description className="sr-only">
                    {typeof title === "string" ? title : ""}
                  </RadixDialog.Description>
                )}
              </div>
              <RadixDialog.Close asChild>
                <IconButton label={closeLabel} size="sm" variant="ghost">
                  <X size={14} />
                </IconButton>
              </RadixDialog.Close>
            </div>
            {children && <div className="mt-4">{children}</div>}
            {footer && (
              <div className="mt-6 flex justify-end gap-2">{footer}</div>
            )}
          </RadixDialog.Content>
        </RadixDialog.Overlay>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
}

/* ---------- DangerConfirm (destructive-action confirmation: can require typing specific text to unlock) ----------
   Same interface as before, skin swapped for Dialog. **Always mounted** (open
   is always true): callers use conditional rendering to control whether it
   appears, which works just as well as Dialog's open prop, and old call sites
   don't need to change. */
export function DangerConfirm({
  title,
  hint,
  requireText,
  confirmLabel,
  cancelLabel,
  busy,
  onConfirm,
  onCancel,
}: {
  title: string;
  hint: string;
  /** Text that must be typed exactly to unlock (e.g. a resource name); omit to allow confirming directly */
  requireText?: string;
  confirmLabel: string;
  cancelLabel: string;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const [text, setText] = useState("");
  const unlocked = !requireText || text === requireText;
  return (
    <Dialog
      open
      onOpenChange={(o) => {
        if (!o) onCancel();
      }}
      title={<span className="text-danger">{title}</span>}
      description={hint}
      closeLabel={cancelLabel}
      width="sm"
      footer={
        <>
          <Button variant="secondary" size="sm" onClick={onCancel}>
            {cancelLabel}
          </Button>
          <Button
            variant="danger"
            size="sm"
            disabled={!unlocked || busy}
            onClick={onConfirm}
          >
            {confirmLabel}
          </Button>
        </>
      }
    >
      {requireText && (
        <Input
          autoFocus
          className="w-full"
          placeholder={requireText}
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
      )}
    </Dialog>
  );
}
