import { Toaster as Sonner } from 'sonner';

export function Toaster() {
  return (
    <Sonner
      position="bottom-right"
      // Modal dialogs disable pointer events outside themselves; toasts must stay clickable.
      style={{ pointerEvents: 'auto' }}
      gap={8}
      offset={20}
      toastOptions={{
        classNames: {
          toast: '!rounded-xl !border !border-border !bg-white !text-ink !shadow-menu !font-sans !text-[14px] !py-3',
          description: '!text-ink-muted',
        },
      }}
    />
  );
}
