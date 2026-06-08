import { invoke } from '@tauri-apps/api/core';
import type { Message, Settings, DailyCost, AnalysisJson, DocInfo } from './types';

export const api = {
  listRecentMessages: (limit = 50) => invoke<Message[]>('list_recent_messages', { limit }),
  getSettings: () => invoke<Settings>('get_settings'),
  saveSettings: (value: Settings) => invoke<void>('save_settings', { value }),
  getApiKeySet: () => invoke<boolean>('get_api_key_set'),
  setApiKey: (value: string) => invoke<void>('set_api_key', { value }),
  clearApiKey: () => invoke<void>('clear_api_key'),
  costToday: () => invoke<DailyCost>('cost_today'),
  saveScreenshot: (base64Png: string, filenameHint?: string) =>
    invoke<string>('save_screenshot', { base64Png, filenameHint }),
  analyze: (req: { user_text: string; screenshot_path?: string; symbol_hint?: string }) =>
    invoke<{ message_id: number }>('analyze', { req }),
  ingestPath: (path: string) => invoke<number>('ingest_path', { path }),
  listDocuments: () => invoke<DocInfo[]>('list_documents'),
  deleteDocument: (docPath: string) => invoke<void>('delete_document', { docPath }),
  embeddingsReady: () => invoke<boolean>('embeddings_ready'),
};

export type { Message, Settings, DailyCost, AnalysisJson, DocInfo };
