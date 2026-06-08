<script lang="ts">
  import type { AnalysisJson } from '../types';
  export let a: AnalysisJson;
  $: actionColor = a.action === 'buy' ? 'bg-green-600' : a.action === 'sell' ? 'bg-red-600' : 'bg-zinc-600';
  $: probPct = Math.round(a.probability_up * 100);
</script>

<div class="rounded-xl border border-zinc-700 p-3 my-2 bg-zinc-900">
  <div class="flex items-center justify-between">
    <span class={`px-2 py-1 rounded text-xs font-semibold uppercase ${actionColor}`}>{a.action}</span>
    <span class="text-sm text-zinc-300">P(up) <span class="font-semibold">{probPct}%</span> · {a.horizon}</span>
  </div>
  {#if a.stop_loss_pct != null || a.take_profit_pct != null}
    <div class="mt-2 text-xs text-zinc-400">
      SL: {a.stop_loss_pct ?? '—'}% · TP: {a.take_profit_pct ?? '—'}%
    </div>
  {/if}
  {#if a.key_signals?.length}
    <ul class="mt-2 text-xs text-zinc-300 list-disc list-inside">
      {#each a.key_signals as s}<li>{s}</li>{/each}
    </ul>
  {/if}
  {#if a.citations?.length}
    <div class="mt-2 text-xs text-zinc-500">
      Cited: {a.citations.map((c) => c.page ? `${c.doc}#${c.page}` : c.doc).join(', ')}
    </div>
  {/if}
</div>
