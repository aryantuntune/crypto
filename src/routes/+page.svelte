<script lang="ts">
  import { onMount } from 'svelte';
  import ChatPanel from '../lib/components/ChatPanel.svelte';
  import LibraryView from '../lib/components/LibraryView.svelte';
  import SettingsView from '../lib/components/SettingsView.svelte';
  import { errorBanner, clearError } from '../lib/stores/error';
  import { refreshAll } from '../lib/stores/settings';
  import { pollEmbeddingsReady } from '../lib/stores/chat';
  type Tab = 'chat' | 'library' | 'settings';
  let tab: Tab = 'chat';

  onMount(async () => {
    await refreshAll();
    await pollEmbeddingsReady();
  });

  function goSettings() {
    tab = 'settings';
  }
</script>

<div class="flex flex-col h-screen">
  <header class="flex items-center gap-2 px-3 py-2 border-b border-zinc-800 bg-zinc-950">
    <span class="font-semibold text-sm">CogniTrade</span>
    <nav class="ml-auto text-xs flex gap-3">
      <button class:underline={tab === 'chat'} on:click={() => (tab = 'chat')}>Chat</button>
      <button class:underline={tab === 'library'} on:click={() => (tab = 'library')}>Library</button>
      <button class:underline={tab === 'settings'} on:click={() => (tab = 'settings')}>Settings</button>
    </nav>
  </header>
  {#if $errorBanner}
    <div class="flex items-start gap-2 px-3 py-2 bg-red-900/70 border-b border-red-700 text-sm text-red-100">
      <span class="flex-1">{$errorBanner}</span>
      <button on:click={clearError} class="text-red-200 hover:text-white text-xs font-semibold">Dismiss</button>
    </div>
  {/if}
  <main class="flex-1 overflow-hidden">
    {#if tab === 'chat'}<ChatPanel onGoSettings={goSettings} />{/if}
    {#if tab === 'library'}<LibraryView />{/if}
    {#if tab === 'settings'}<SettingsView />{/if}
  </main>
</div>
