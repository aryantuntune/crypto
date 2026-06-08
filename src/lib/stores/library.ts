import { writable } from 'svelte/store';
import { api } from '../ipc';
import { pushError } from './error';
import type { DocInfo } from '../types';

export const ingestStatus = writable<{ path: string; chunks: number; ok: boolean; err?: string }[]>([]);

export const documents = writable<DocInfo[]>([]);
export const docsLoading = writable<boolean>(false);
export const docsLoaded = writable<boolean>(false);

export async function refreshDocuments() {
  docsLoading.set(true);
  try {
    documents.set(await api.listDocuments());
    docsLoaded.set(true);
  } catch (e) {
    pushError(e);
  } finally {
    docsLoading.set(false);
  }
}

export async function ingest(path: string) {
  try {
    const chunks = await api.ingestPath(path);
    ingestStatus.update((arr) => [...arr, { path, chunks, ok: true }]);
    await refreshDocuments();
  } catch (e) {
    ingestStatus.update((arr) => [...arr, { path, chunks: 0, ok: false, err: String(e) }]);
    pushError(e);
  }
}

export async function removeDocument(docPath: string) {
  try {
    await api.deleteDocument(docPath);
    await refreshDocuments();
  } catch (e) {
    pushError(e);
  }
}
