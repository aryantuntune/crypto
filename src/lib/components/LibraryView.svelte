<script lang="ts">
  import { onMount } from 'svelte';
  import { open } from '@tauri-apps/plugin-dialog';
  import { documents, docsLoading, docsLoaded, ingestStatus, ingest, removeDocument, refreshDocuments } from '../stores/library';

  let busy = false;

  onMount(refreshDocuments);

  async function pickFiles() {
    const sel = await open({
      multiple: true,
      filters: [{ name: 'PDF and images', extensions: ['pdf', 'png', 'jpg', 'jpeg', 'webp'] }],
    });
    const list = Array.isArray(sel) ? sel : sel ? [sel] : [];
    if (list.length === 0) return;
    busy = true;
    for (const p of list) await ingest(p);
    busy = false;
  }

  function basename(p: string): string {
    return p.split(/[\\/]/).pop() ?? p;
  }
</script>

<div class="p-3 space-y-3">
  <button on:click={pickFiles} disabled={busy} class="rounded bg-blue-600 hover:bg-blue-500 disabled:bg-zinc-700 disabled:cursor-not-allowed px-3 py-2 text-sm">
    {busy ? 'Adding…' : 'Add document'}
  </button>
  <p class="text-xs text-zinc-400">PDFs and chart images go here. Files are also picked up automatically when dropped into ~/CogniTrade/library.</p>

  {#if $ingestStatus.length}
    <div class="space-y-1 text-xs">
      {#each $ingestStatus as s}
        <div class="text-zinc-300">
          {s.ok ? '✅' : '❌'} {basename(s.path)} {s.ok ? `(${s.chunks} chunks)` : `— ${s.err}`}
        </div>
      {/each}
    </div>
  {/if}

  <section>
    <h3 class="font-semibold text-sm mb-1">Ingested documents</h3>
    {#if $docsLoading && !$docsLoaded}
      <div class="text-xs text-zinc-400">Loading…</div>
    {:else if $documents.length === 0}
      <div class="text-xs text-zinc-500 border border-dashed border-zinc-700 rounded px-3 py-4 text-center">
        No documents yet. Add a trading book, research paper, or chart image to ground future analyses.
      </div>
    {:else}
      <div class="divide-y divide-zinc-800 border border-zinc-800 rounded">
        {#each $documents as d (d.doc_path)}
          <div class="flex items-center gap-2 px-2 py-2 text-xs">
            <div class="flex-1 min-w-0">
              <div class="text-zinc-200 truncate" title={d.doc_path}>{basename(d.doc_path)}</div>
              <div class="text-zinc-500">{d.doc_type} · {d.chunks} chunks</div>
            </div>
            <button on:click={() => removeDocument(d.doc_path)} class="px-2 py-1 rounded bg-red-700 hover:bg-red-600 text-white">Delete</button>
          </div>
        {/each}
      </div>
    {/if}
  </section>
</div>
