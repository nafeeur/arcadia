# Arcadia interface

Arcadia replaces Utopia's dark glass presentation with a quiet editorial workspace. This is an intentional product redesign; the original feature architecture remains.

## Visual language

- Evergreen navigation rail (`#182d28`), ivory canvas, warm paper panels, muted green controls and restrained gold identity marks.
- Marcellus for the brand and major page headings; the existing sans-serif stack for dense knowledge work.
- Flat panels with fine borders and small radii. Avoid decorative glow, heavy gradients and large floating shadows.
- Persistent grouped navigation: the overview, changes, answer ledger and historical evidence form the daily workflow; existing knowledge and governance tools remain reachable.
- Real counts and explicit empty states. Never populate a new workspace with fabricated people, documents, activity or measurements.

## Components and behavior

Shared tokens live in `src/styles.css`; Arcadia layout and page components live in `src/arcadia.css`. Use the existing `src/ui` primitives for inputs, buttons, menus and dialogs. Keep text in both `src/i18n/en.ts` and `zh.ts`.

Review pages use an inbox and inspector, with the retained original beside the proposal. Small screens stack these regions and allow horizontal navigation scrolling. The knowledge-base switch remains available. Keyboard focus is visible; reduced-motion preferences disable decorative motion.

Historical record time is labeled UTC and separate from the graph's world-time controls. Answer reruns clearly state their document-only scope. Citation structure must never be labeled factual verification. Private answer dependencies belong only to the current user.

The source compiles and passes the style guard. Browser visual/accessibility testing has not been performed in this build session.
