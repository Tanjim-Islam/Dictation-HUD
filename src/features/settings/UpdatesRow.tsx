import { checkForUpdates, installUpdate, useUpdateState } from './updater';
import { Download, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Spinner } from '@/components/ui/misc';
import { Row } from './parts';

export function UpdatesRow() {
  const s = useUpdateState();
  const description =
    s.status === 'checking'
      ? 'Checking for updates...'
      : s.status === 'uptodate'
        ? 'You have the latest version.'
        : s.status === 'available'
          ? `Version ${s.version} is available.`
          : s.status === 'downloading'
            ? `Downloading ${s.progress}%`
            : s.status === 'ready'
              ? 'Restarting to finish the update...'
              : s.status === 'error'
                ? `Update check failed: ${s.message}`
                : 'Updates are signed and installed from GitHub releases.';
  return (
    <Row
      title="Updates"
      description={description}
      control={
        s.status === 'available' ? (
          <Button variant="primary" onClick={installUpdate}>
            <Download strokeWidth={1.75} />
            Install and restart
          </Button>
        ) : (
          <Button variant="secondary" onClick={() => void checkForUpdates()} disabled={s.status === 'checking' || s.status === 'downloading'}>
            {s.status === 'checking' || s.status === 'downloading' ? <Spinner /> : <RefreshCw strokeWidth={1.75} />}
            Check for updates
          </Button>
        )
      }
    />
  );
}
