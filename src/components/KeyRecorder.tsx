import { useState, useEffect, useRef } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { Edit3 } from 'lucide-react';

interface KeyRecorderProps {
  value: string;
  onChange: (combo: string) => void;
  onSave: () => void;
}

// Normalize key names for cross-platform consistency
function normalizeKey(key: string): string {
  const map: Record<string, string> = {
    'Control': 'Ctrl',
    'Meta': 'Cmd',
    'AltGraph': 'Alt',
    ' ': 'Space',
  };
  return map[key] || key;
}

// Convert event to modifier + key array
function eventToKeyArray(e: KeyboardEvent): string[] {
  const parts: string[] = [];

  // Add modifiers in consistent order
  if (e.ctrlKey) parts.push('Ctrl');
  if (e.metaKey) parts.push('Cmd');
  if (e.shiftKey) parts.push('Shift');
  if (e.altKey) parts.push('Alt');

  // Add the main key (if not a modifier itself)
  const key = normalizeKey(e.key);
  if (!['Control', 'Shift', 'Alt', 'Meta', 'Cmd', 'Ctrl', 'AltGraph'].includes(key)) {
    parts.push(key.length === 1 ? key.toUpperCase() : key);
  }

  return parts;
}

export function KeyRecorder({ value, onChange, onSave }: KeyRecorderProps) {
  const [isRecording, setIsRecording] = useState(false);
  const [recordedKeys, setRecordedKeys] = useState<string[]>([]);
  const recordingRef = useRef(false);
  const recordedKeysRef = useRef<string[]>([]);
  const containerRef = useRef<HTMLDivElement>(null);
  const keyUpTimeoutRef = useRef<number | null>(null);

  // Set up event listeners once on mount
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (!recordingRef.current) return;

      // Prevent default behavior for all keys during recording
      e.preventDefault();
      e.stopPropagation();

      // Clear any pending finalization
      if (keyUpTimeoutRef.current) {
        clearTimeout(keyUpTimeoutRef.current);
        keyUpTimeoutRef.current = null;
      }

      const keys = eventToKeyArray(e);

      console.log('[KeyRecorder] KeyDown - keys captured:', keys, 'length:', keys.length);

      // Always reflect currently held keys immediately for visual feedback
      if (keys.length > 0) {
        recordedKeysRef.current = keys;
        setRecordedKeys([...keys]); // trigger re-render
      }
    };

    const handleKeyUp = (e: KeyboardEvent) => {
      if (!recordingRef.current) return;

      e.preventDefault();
      e.stopPropagation();

      console.log('[KeyRecorder] KeyUp detected, current recorded keys:', recordedKeysRef.current);

      // Finalize recording after a short delay to ensure all keys are captured
      if (keyUpTimeoutRef.current) {
        clearTimeout(keyUpTimeoutRef.current);
      }

      keyUpTimeoutRef.current = window.setTimeout(() => {
        console.log('[KeyRecorder] Finalizing combo, recorded keys:', recordedKeysRef.current);
        if (recordedKeysRef.current.length >= 2) {
          const finalCombo = recordedKeysRef.current.join('+');
          console.log('[KeyRecorder] Calling onChange with:', finalCombo);
          onChange(finalCombo);
        } else {
          console.log('[KeyRecorder] Combo too short, cancelling recording');
        }
        // Exit recording mode regardless, so UI never gets stuck
        setIsRecording(false);
        recordingRef.current = false;
        // Keep the last valid value shown when not recording
      }, 80);
    };

    const handleClickOutside = (e: MouseEvent) => {
      if (!recordingRef.current) return;
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        if (recordedKeysRef.current.length >= 2) {
          const finalCombo = recordedKeysRef.current.join('+');
          onChange(finalCombo);
        }
        setIsRecording(false);
        recordingRef.current = false;
        recordedKeysRef.current = [];
        setRecordedKeys([]);
      }
    };

    window.addEventListener('keydown', handleKeyDown, { capture: true });
    window.addEventListener('keyup', handleKeyUp, { capture: true });
    document.addEventListener('mousedown', handleClickOutside);

    return () => {
      window.removeEventListener('keydown', handleKeyDown, { capture: true });
      window.removeEventListener('keyup', handleKeyUp, { capture: true });
      document.removeEventListener('mousedown', handleClickOutside);
      if (keyUpTimeoutRef.current) {
        clearTimeout(keyUpTimeoutRef.current);
      }
    };
  }, [onChange]);

  const startRecording = () => {
    console.log('[KeyRecorder] Start recording button clicked');
    recordedKeysRef.current = [];
    setRecordedKeys([]);
    recordingRef.current = true;
    setIsRecording(true);
    console.log('[KeyRecorder] Recording state set, recordingRef:', recordingRef.current, 'isRecording will update on next render');
  };

  const displayKeys = isRecording ? recordedKeys : (value ? value.split('+') : []);

  console.log('[KeyRecorder] Render - isRecording:', isRecording, 'recordedKeys:', recordedKeys, 'displayKeys:', displayKeys, 'value:', value);

  return (
    <div ref={containerRef} className="flex gap-2 items-center">
      <div
        className={`flex-1 px-3 py-2 bg-neutral-900 rounded border min-h-[42px] flex items-center gap-1.5 ${
          isRecording ? 'border-accent ring-2 ring-accent/20' : 'border-neutral-700'
        }`}
      >
        {/* Avoid mode="wait" because the recording indicator uses looping animation */}
        <AnimatePresence>
          {isRecording && displayKeys.length === 0 ? (
            <motion.span
              key="recording"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="text-sm animate-pulse"
              style={{
                color: '#d66',
              }}
            >
              Recording...
            </motion.span>
          ) : displayKeys.length > 0 ? (
            <motion.div
              key="keys"
              initial={{ opacity: 0, scale: 0.95 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.95 }}
              className="flex items-center gap-1.5 flex-wrap"
            >
              {displayKeys.map((key, idx) => (
                <motion.span
                  key={`${key}-${idx}`}
                  initial={{ opacity: 0, y: -4 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={{ delay: idx * 0.05 }}
                  className="px-2 py-0.5 bg-[#f2f1ea] text-[#1a1a1a] rounded text-sm font-medium"
                >
                  {key}
                </motion.span>
              ))}
            </motion.div>
          ) : (
            <motion.span
              key="empty"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="text-sm text-muted"
            >
              No hotkey set
            </motion.span>
          )}
        </AnimatePresence>
      </div>

      <button
        onClick={startRecording}
        disabled={isRecording}
        className="px-3 py-2 bg-neutral-800 rounded border border-neutral-700 hover:bg-neutral-700 transition disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2"
        aria-label="Edit hotkey"
        title="Edit hotkey"
      >
        <Edit3 size={16} />
        <span className="text-sm">Edit</span>
      </button>

      <button
        onClick={onSave}
        disabled={!value || value.trim() === ''}
        className="px-3 py-2 bg-accent text-black rounded hover:brightness-95 active:brightness-90 transition disabled:opacity-50 disabled:cursor-not-allowed"
        aria-label="Save hotkey"
        title="Save hotkey"
      >
        <span className="text-sm font-medium">Save</span>
      </button>
    </div>
  );
}
