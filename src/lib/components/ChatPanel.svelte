<script lang="ts">
  import { onMount } from 'svelte';
  import { messages, streaming, analysisResult, isAnalyzing, embeddingsReady, refreshMessages, initEvents, sendAnalyze } from '../stores/chat';
  import { apiKeySet } from '../stores/settings';
  import MessageBubble from './MessageBubble.svelte';
  import PredictionCard from './PredictionCard.svelte';
  import ScreenshotPaste from './ScreenshotPaste.svelte';

  export let onGoSettings: (() => void) | undefined = undefined;

  let text = '';
  let pendingScreenshot: string | undefined;
  let symbolHint = '';

  onMount(async () => {
    await refreshMessages();
    await initEvents();
  });

  async function send() {
    if (!text.trim() && !pendingScreenshot) return;
    const t = text;
    const ss = pendingScreenshot;
    const sh = symbolHint || undefined;
    text = ''; pendingScreenshot = undefined;
    await sendAnalyze({ text: t, screenshotPath: ss, symbolHint: sh });
  }

  function onPaste(e: CustomEvent<{ path: string }>) { pendingScreenshot = e.detail.path; }
</script>

<ScreenshotPaste on:saved={onPaste} />

<div class="flex flex-col h-full">
  <div class="flex-1 overflow-y-auto px-3 py-2">
    {#if $messages.length === 0 && !$streaming && !$analysisResult}
      <div class="h-full flex flex-col items-center justify-center text-center text-zinc-500 px-6">
        <div class="text-3xl mb-2">📈</div>
        <p class="text-sm font-medium text-zinc-300">No analyses yet</p>
        <p class="text-xs mt-1 max-w-xs">Paste a chart screenshot with Ctrl+V (or just describe a setup) and hit Analyze to get an advisory read.</p>
      </div>
    {/if}
    {#each $messages as m (m.id)}
      <MessageBubble {m} />
    {/each}
    {#if $streaming}
      <div class="my-2 flex justify-start">
        <div class="max-w-[85%] rounded-lg px-3 py-2 text-sm bg-zinc-800 text-zinc-100">
          <pre class="whitespace-pre-wrap font-sans">{$streaming}</pre>
        </div>
      </div>
    {/if}
    {#if $analysisResult}
      <PredictionCard a={$analysisResult} />
    {/if}
  </div>

  <div class="border-t border-zinc-800 p-2 space-y-2">
    {#if pendingScreenshot}
      <div class="text-xs text-zinc-400">📎 screenshot ready: {pendingScreenshot.split('\\').pop()}</div>
    {/if}
    {#if !$apiKeySet}
      <div class="text-xs text-amber-300 bg-amber-900/30 border border-amber-800 rounded px-2 py-1">
        No API key set. Add one in the
        <button on:click={() => onGoSettings && onGoSettings()} class="underline font-medium hover:text-amber-100">Settings</button>
        tab to enable analysis.
      </div>
    {:else if !$embeddingsReady}
      <div class="text-xs text-zinc-400">Preparing local search model…</div>
    {/if}
    <input bind:value={symbolHint} placeholder="symbol (e.g. BTCUSDT)" class="w-full bg-zinc-900 border border-zinc-700 rounded px-2 py-1 text-sm" />
    <textarea bind:value={text} rows="2" placeholder="ask about the chart… (paste screenshot with Ctrl+V)" class="w-full bg-zinc-900 border border-zinc-700 rounded px-2 py-1 text-sm"></textarea>
    <button on:click={send} disabled={$isAnalyzing || !$apiKeySet} class="w-full rounded bg-blue-600 hover:bg-blue-500 disabled:bg-zinc-700 disabled:cursor-not-allowed text-white py-2 text-sm font-medium">
      {$isAnalyzing ? 'Analyzing…' : 'Analyze'}
    </button>
  </div>
</div>
