// "Morph in place" (FLIP) for header popover panels: the panel covers the
// trigger button's original position, the first frame is clamped to the
// button's actual bounds (999px border radius), and the next frame
// transitions to the panel's full shape — the button "grows into" the panel;
// closing reverses that shrink.
//
// **Factored into one shared hook** rather than having the user menu and
// alerts each write their own: these two panels sit right next to each
// other, and even a small difference in duration or easing shows up the
// moment you click back and forth between them.
import { useEffect, useLayoutEffect, useRef, useState } from "react";

const OPEN_MS = 260;
const CLOSE_MS = 190;

function reduced(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * Returns `{ open, setOpen, close, anchorRef, panelRef }`.
 *
 * `close()` plays the collapse animation before unmounting; to close
 * immediately (e.g. navigation happened), call `setOpen(false)` directly.
 * Outside-click and Esc are already wired up, attached to `rootRef`.
 */
export function usePopoverFlip<A extends HTMLElement, P extends HTMLElement>(
  /** The morph's anchor corner. **Write whichever side the panel hugs**: a
   *  panel on the right of the header hugs the top-right corner; a panel
   *  hugging the left (e.g. the legend's "+N classes") should say "top left",
   *  otherwise it grows leftward from the right edge and looks like it flew
   *  in from somewhere else */
  origin: "top right" | "top left" | "bottom left" = "top right",
) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const anchorRef = useRef<A>(null);
  const panelRef = useRef<P>(null);
  const closingRef = useRef(false);

  useLayoutEffect(() => {
    if (!open) return;
    const panel = panelRef.current;
    const anchor = anchorRef.current;
    if (!panel || !anchor || reduced()) return;
    const a = anchor.getBoundingClientRect();
    const p = panel.getBoundingClientRect();
    if (p.width < 1 || p.height < 1) return;
    panel.style.transformOrigin = origin;
    panel.style.transform = `scale(${a.width / p.width}, ${a.height / p.height})`;
    panel.style.borderRadius = "999px";
    panel.style.opacity = "0.35";
    let done: number | undefined;
    const raf = requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        panel.style.transition = `transform ${OPEN_MS}ms cubic-bezier(0.16,1,0.3,1), border-radius ${OPEN_MS}ms cubic-bezier(0.16,1,0.3,1), opacity 0.18s ease`;
        panel.style.transform = "scale(1, 1)";
        panel.style.borderRadius = "12px";
        panel.style.opacity = "1";
        // Once the animation finishes, **fully clear** the inline styles —
        // don't leave a `scale(1,1)` behind. An identity transform looks
        // harmless, but it still creates a compositing layer, which snaps
        // absolutely-positioned children inside the panel to a device pixel
        // once — a 0.67px offset at DPR 1.5 — and the close button needs to
        // **exactly overlap** the button that triggered it, where even one
        // physical pixel of difference is visible
        done = window.setTimeout(() => {
          panel.style.transition = "";
          panel.style.transform = "";
          panel.style.borderRadius = "";
          panel.style.opacity = "";
          panel.style.transformOrigin = "";
        }, OPEN_MS + 20);
      }),
    );
    return () => {
      cancelAnimationFrame(raf);
      if (done !== undefined) window.clearTimeout(done);
    };
  }, [open, origin]);

  const close = () => {
    const panel = panelRef.current;
    const anchor = anchorRef.current;
    if (closingRef.current) return;
    if (!panel || !anchor || reduced()) {
      setOpen(false);
      return;
    }
    closingRef.current = true;
    panel.style.transformOrigin = origin;
    const a = anchor.getBoundingClientRect();
    // offsetWidth/Height are layout dimensions, unaffected by the current
    // transform — getBoundingClientRect would return the already-scaled
    // value, shrinking further each time
    panel.style.transition = `transform ${CLOSE_MS}ms cubic-bezier(0.5,0,0.9,0.4), border-radius ${CLOSE_MS}ms cubic-bezier(0.5,0,0.9,0.4), opacity 0.16s ease`;
    panel.style.transform = `scale(${a.width / panel.offsetWidth}, ${a.height / panel.offsetHeight})`;
    panel.style.borderRadius = "999px";
    panel.style.opacity = "0.3";
    window.setTimeout(() => {
      closingRef.current = false;
      setOpen(false);
    }, CLOSE_MS);
  };

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node))
        close();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  return { open, setOpen, close, rootRef, anchorRef, panelRef };
}
