import { useEffect, useState } from 'react';

function startOfToday() {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/**
 * Local midnight of the current day, updated when the date changes. A timer fires just after
 * midnight, and focus or visibility changes re-check, since timers drift while the PC sleeps.
 */
export function useToday() {
  const [today, setToday] = useState(startOfToday);
  useEffect(() => {
    let timer = 0;
    const check = () => {
      setToday(startOfToday());
      window.clearTimeout(timer);
      const next = new Date();
      next.setHours(24, 0, 1, 0);
      timer = window.setTimeout(check, next.getTime() - Date.now());
    };
    check();
    const onVisible = () => document.visibilityState === 'visible' && check();
    window.addEventListener('focus', check);
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      window.clearTimeout(timer);
      window.removeEventListener('focus', check);
      document.removeEventListener('visibilitychange', onVisible);
    };
  }, []);
  return today;
}
