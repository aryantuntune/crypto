import { writable, get } from 'svelte/store';
import { listen } from '@tauri-apps/api/event';
import { api } from '../ipc';
import { pushError } from './error';
import type { Message, AnalysisJson } from '../types';

export const messages = writable<Message[]>([]);
export const streaming = writable<string>('');     // assistant text being streamed
export const analysisResult = writable<AnalysisJson | null>(null);
export const isAnalyzing = writable<boolean>(false);
export const embeddingsReady = writable<boolean>(false);

let embeddingsPoll: ReturnType<typeof setInterval> | null = null;

// Poll embeddings_ready() until the local BGE model finishes loading/downloading.
export async function pollEmbeddingsReady() {
  if (get(embeddingsReady)) return;
  try {
    if (await api.embeddingsReady()) {
      embeddingsReady.set(true);
      return;
    }
  } catch {
    // ignore transient errors; keep polling
  }
  if (embeddingsPoll) return;
  embeddingsPoll = setInterval(async () => {
    try {
      if (await api.embeddingsReady()) {
        embeddingsReady.set(true);
        if (embeddingsPoll) {
          clearInterval(embeddingsPoll);
          embeddingsPoll = null;
        }
      }
    } catch {
      // ignore transient errors; keep polling
    }
  }, 2000);
}

export async function refreshMessages() {
  messages.set(await api.listRecentMessages(50));
}

let unlistenChunk: (() => void) | null = null;
let unlistenDone: (() => void) | null = null;

export async function initEvents() {
  if (unlistenChunk) return;
  unlistenChunk = await listen<string>('analysis_chunk', (e) => {
    streaming.update((s) => s + e.payload);
  });
  unlistenDone = await listen<{ message_id: number; analysis: AnalysisJson | null }>(
    'analysis_done',
    async (e) => {
      analysisResult.set(e.payload.analysis);
      streaming.set('');
      isAnalyzing.set(false);
      await refreshMessages();
    }
  );
}

export async function sendAnalyze(opts: { text: string; screenshotPath?: string; symbolHint?: string }) {
  isAnalyzing.set(true);
  streaming.set('');
  analysisResult.set(null);
  try {
    await api.analyze({
      user_text: opts.text,
      screenshot_path: opts.screenshotPath,
      symbol_hint: opts.symbolHint,
    });
  } catch (e) {
    isAnalyzing.set(false);
    streaming.set('');
    pushError(e);
  } finally {
    // Done event will refresh; but if it failed before any event, refresh anyway:
    if (get(isAnalyzing)) {
      isAnalyzing.set(false);
      await refreshMessages();
    }
  }
}
