<script lang="ts">
  import { onMount } from 'svelte';
  import { settings, apiKeySet, cost, refreshAll, save, setKey, clearKey } from '../stores/settings';
  import { pushError } from '../stores/error';

  let key = '';
  let cap = 2;
  let model = 'claude-sonnet-4-6';
  let modelExtract = '';
  let capError = '';

  onMount(async () => {
    await refreshAll();
    if ($settings) {
      cap = $settings.daily_cost_cap_usd;
      model = $settings.model_main;
      modelExtract = $settings.model_extract;
    }
  });

  async function persist() {
    if (!$settings) return;
    capError = '';
    if (!Number.isFinite(cap) || cap <= 0) {
      capError = 'Daily cap must be greater than 0.';
      return;
    }
    try {
      await save({
        ...$settings,
        daily_cost_cap_usd: cap,
        model_main: model,
        model_extract: modelExtract,
      });
    } catch (e) {
      // surfaced via banner already in store; keep inline hint too
      pushError(e);
    }
  }
</script>

<div class="p-3 space-y-4 text-sm">
  <section>
    <h3 class="font-semibold mb-1">API key</h3>
    {#if $apiKeySet}
      <div class="text-zinc-300">key is set</div>
      <button on:click={clearKey} class="mt-1 px-2 py-1 rounded bg-red-600 hover:bg-red-500 text-xs">Clear</button>
    {:else}
      <input type="password" bind:value={key} placeholder="sk-ant-…" class="w-full bg-zinc-900 border border-zinc-700 rounded px-2 py-1" />
      <button on:click={() => setKey(key)} class="mt-1 px-2 py-1 rounded bg-blue-600 hover:bg-blue-500 text-xs">Save key</button>
    {/if}
  </section>

  <section>
    <h3 class="font-semibold mb-1">Daily cap (USD)</h3>
    <input type="number" min="0.5" step="0.5" bind:value={cap} class="w-32 bg-zinc-900 border border-zinc-700 rounded px-2 py-1" />
    {#if capError}
      <div class="text-xs text-red-400 mt-1">{capError}</div>
    {/if}
    {#if $cost}
      <div class="text-xs text-zinc-400 mt-1">today: ${$cost.cost_usd.toFixed(3)}</div>
    {/if}
  </section>

  <section>
    <h3 class="font-semibold mb-1">Main model</h3>
    <select bind:value={model} class="bg-zinc-900 border border-zinc-700 rounded px-2 py-1">
      <option value="claude-sonnet-4-6">claude-sonnet-4-6 (default)</option>
      <option value="claude-opus-4-7">claude-opus-4-7 (slower, costlier)</option>
    </select>
  </section>

  <section>
    <h3 class="font-semibold mb-1">Extraction model</h3>
    <input bind:value={modelExtract} placeholder="claude-…" class="w-full bg-zinc-900 border border-zinc-700 rounded px-2 py-1" />
    <p class="text-xs text-zinc-500 mt-1">Used for cheaper chart/text extraction passes.</p>
  </section>

  <section>
    <h3 class="font-semibold mb-1">Library path</h3>
    <div class="text-xs text-zinc-400 break-all">{$settings?.library_path ?? '— (default ~/CogniTrade/library)'}</div>
  </section>

  <section>
    <h3 class="font-semibold mb-1">Global hotkey</h3>
    <div class="text-xs text-zinc-400">{$settings?.hotkey ?? '—'}</div>
  </section>

  <button on:click={persist} class="px-3 py-2 rounded bg-blue-600 hover:bg-blue-500">Save</button>
</div>
