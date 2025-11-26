import { register, isRegistered, unregisterAll } from '@tauri-apps/plugin-global-shortcut';
import { invoke } from '@tauri-apps/api/core';

// Debounce variables to prevent rapid consecutive hotkey presses
let isProcessingHotkey = false;
let lastHotkeyTime = 0;
const HOTKEY_COOLDOWN_MS = 500; // Minimum time between hotkey actions

export async function initGlobalHotkey(combo: string): Promise<boolean> {
  const logMsg = (msg: string) => {
    console.log(msg);
    invoke('log_to_terminal', { message: msg }).catch(() => {});
  };

  logMsg('=== initGlobalHotkey START ===');
  logMsg('Combo to register: ' + combo);

  if (!combo || combo.trim() === '') {
    logMsg('❌ initGlobalHotkey: invalid combo ' + combo);
    return false;
  }

  try {
    logMsg('🔄 Unregistering all existing shortcuts...');
    await unregisterAll();
    logMsg('✅ All shortcuts unregistered');

    logMsg('🔍 Checking if combo is already registered: ' + combo);
    const already = await isRegistered(combo);
    logMsg('Already registered? ' + already);

    if (!already) {
      logMsg('📝 Attempting to register hotkey: ' + combo);
      await register(combo, async () => {
        logMsg('🔥🔥🔥 GLOBAL HOTKEY PRESSED 🔥🔥🔥');
        logMsg('Combo that was pressed: ' + combo);

        // Debounce: ignore rapid consecutive presses
        const now = Date.now();
        const timeSinceLastPress = now - lastHotkeyTime;

        if (isProcessingHotkey) {
          logMsg('⏸️ Hotkey already processing, ignoring this press');
          return;
        }

        if (timeSinceLastPress < HOTKEY_COOLDOWN_MS) {
          logMsg(`⏸️ Hotkey pressed too quickly (${timeSinceLastPress}ms < ${HOTKEY_COOLDOWN_MS}ms), ignoring`);
          return;
        }

        isProcessingHotkey = true;
        lastHotkeyTime = now;

        // Check if dictation is currently active by checking HUD window visibility
        try {
          const isActive = await invoke<boolean>('is_dictation_active');
          logMsg('Dictation active: ' + isActive);

          if (isActive) {
            logMsg('🛑 Dictation is active, sending stop signal...');
            await invoke('trigger_stop_dictation');
            logMsg('✅ Stop signal sent');
          } else {
            logMsg('▶️ Dictation is inactive, starting...');
            await invoke('start_dictation');
            logMsg('✅ start_dictation invoke completed');
          }
        } catch (e) {
          logMsg('❌ Hotkey handler FAILED: ' + String(e));
          logMsg('Error details: ' + JSON.stringify(e, null, 2));
        } finally {
          // Release the lock after a short delay to prevent immediate re-trigger
          setTimeout(() => {
            isProcessingHotkey = false;
            logMsg('🔓 Hotkey processing lock released');
          }, 300);
        }
      });
      logMsg('✅ Hotkey registered successfully: ' + combo);
    } else {
      logMsg('ℹ️ Hotkey already registered: ' + combo);
    }
    logMsg('=== initGlobalHotkey END (success) ===');
    return true;
  } catch (e) {
    const errStr = String(e);

    // If the hotkey is already registered (e.g., due to React StrictMode double-mount), treat as success
    if (errStr.includes('HotKey already registered')) {
      logMsg('ℹ️ Hotkey already registered (likely due to concurrent mount), treating as success');
      logMsg('=== initGlobalHotkey END (already registered) ===');
      return true;
    }

    logMsg('❌❌❌ initGlobalHotkey FAILED for ' + combo);
    logMsg('Error type: ' + (e instanceof Error ? e.constructor.name : typeof e));
    logMsg('Error message: ' + (e instanceof Error ? e.message : String(e)));
    logMsg('Error string: ' + errStr);
    if (e instanceof Error && e.stack) {
      logMsg('Error stack: ' + e.stack);
    }
    logMsg('=== initGlobalHotkey END (error) ===');
    return false;
  }
}

export async function setupGlobalHotkey(combo: string) {
  await initGlobalHotkey(combo);
}

