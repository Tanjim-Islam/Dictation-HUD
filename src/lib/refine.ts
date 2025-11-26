import { invoke } from '@tauri-apps/api/core';

export async function refineText(rawText: string): Promise<string> {
  const refined = await invoke<string>('refine_text', { rawText });
  return refined;
}

