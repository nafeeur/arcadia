/* Tooltip: Radix Tooltip. For things with no visible text (icon buttons, truncated names, a bare number);
   buttons with a visible label don't need it. Native `title` waits a second and styles per-OS — this doesn't. */
import { Tooltip as RadixTooltip } from "radix-ui";
import type { ReactNode } from "react";

export function Tooltip({
  content,
  side = "top",
  children,
}: {
  content: ReactNode;
  side?: "top" | "bottom" | "left" | "right";
  /** The trigger element; must accept a ref and events (Button / IconButton / a native element all work) */
  children: ReactNode;
}) {
  return (
    <RadixTooltip.Provider delayDuration={300} skipDelayDuration={200}>
      <RadixTooltip.Root>
        <RadixTooltip.Trigger asChild>{children}</RadixTooltip.Trigger>
        <RadixTooltip.Portal>
          <RadixTooltip.Content
            side={side}
            sideOffset={6}
            collisionPadding={8}
            className="u-pop u-pop-in z-[60] max-w-xs rounded-lg px-2 py-1 text-fine text-ink-2 shadow-xl"
          >
            {content}
          </RadixTooltip.Content>
        </RadixTooltip.Portal>
      </RadixTooltip.Root>
    </RadixTooltip.Provider>
  );
}
