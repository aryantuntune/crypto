<script lang="ts">
  import { api } from '../ipc';
  import { createEventDispatcher } from 'svelte';
  const dispatch = createEventDispatcher<{ saved: { path: string } }>();
  export let busy = false;

  async function onPaste(e: ClipboardEvent) {
    if (!e.clipboardData) return;
    const item = Array.from(e.clipboardData.items).find((it) => it.type.startsWith('image/'));
    if (!item) return;
    const blob = item.getAsFile();
    if (!blob) return;
    busy = true;
    const buf = await blob.arrayBuffer();
    const b64 = btoa(String.fromCharCode(...new Uint8Array(buf)));
    const path = await api.saveScreenshot(b64, 'paste');
    busy = false;
    dispatch('saved', { path });
  }
</script>

<svelte:window on:paste={onPaste} />
