import { writable } from 'svelte/store';
import { api } from '../ipc';
import { pushError } from './error';
import type { Settings, DailyCost } from '../types';

export const settings = writable<Settings | null>(null);
export const apiKeySet = writable<boolean>(false);
export const cost = writable<DailyCost | null>(null);

export async function refreshAll() {
  try {
    settings.set(await api.getSettings());
    apiKeySet.set(await api.getApiKeySet());
    cost.set(await api.costToday());
  } catch (e) {
    pushError(e);
  }
}

export async function save(s: Settings) {
  try {
    await api.saveSettings(s);
    settings.set(s);
  } catch (e) {
    pushError(e);
    throw e;
  }
}

export async function setKey(k: string) {
  try {
    await api.setApiKey(k);
    apiKeySet.set(true);
  } catch (e) {
    pushError(e);
  }
}

export async function clearKey() {
  try {
    await api.clearApiKey();
    apiKeySet.set(false);
  } catch (e) {
    pushError(e);
  }
}
