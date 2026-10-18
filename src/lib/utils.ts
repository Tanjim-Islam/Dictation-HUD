import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatNumber(n: number) {
  return n.toLocaleString('en-US');
}

/**
 * For modal dialogs: a click on a toast (for example its action button) is not an outside
 * click, so it neither closes the dialog nor gets swallowed.
 */
export function keepToastsInteractive(e: { target: EventTarget | null; preventDefault: () => void }) {
  if (e.target instanceof Element && e.target.closest('[data-sonner-toaster]')) e.preventDefault();
}
