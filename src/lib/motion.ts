// Central motion tokens. Motion follows state; it never drives it.
import type { Transition } from 'motion/react';

export const ease = {
  out: [0.22, 1, 0.36, 1] as const,
  inOut: [0.65, 0, 0.35, 1] as const,
  in: [0.4, 0, 1, 1] as const,
};

export const duration = {
  fast: 0.12,
  base: 0.18,
  slow: 0.28,
};

export const transition = {
  base: { duration: duration.base, ease: ease.out } satisfies Transition,
  fast: { duration: duration.fast, ease: ease.out } satisfies Transition,
  slow: { duration: duration.slow, ease: ease.out } satisfies Transition,
  /** Shared-layout highlights (sidebar selection, tabs). */
  layout: { type: 'spring', stiffness: 520, damping: 42, mass: 0.9 } satisfies Transition,
  /** Small presses and toggles. */
  press: { type: 'spring', stiffness: 700, damping: 36 } satisfies Transition,
};

export const fadeSlide = {
  initial: { opacity: 0, y: 6 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, y: -4 },
};
