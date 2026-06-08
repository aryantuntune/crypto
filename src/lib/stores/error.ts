import { writable } from 'svelte/store';

// Global, dismissible error banner message. `null` means no banner shown.
export const errorBanner = writable<string | null>(null);

export function pushError(e: unknown) {
  const msg = e instanceof Error ? e.message : String(e);
  errorBanner.set(msg);
}

export function clearError() {
  errorBanner.set(null);
}
