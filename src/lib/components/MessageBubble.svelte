<script lang="ts">
  import type { Message } from '../types';
  export let m: Message;
  $: isUser = m.role === 'user';
</script>

<div class={`my-2 flex ${isUser ? 'justify-end' : 'justify-start'}`}>
  <div class={`max-w-[85%] rounded-lg px-3 py-2 text-sm ${isUser ? 'bg-blue-600 text-white' : 'bg-zinc-800 text-zinc-100'}`}>
    {#if m.image_path}
      <img src={'file://' + m.image_path} alt="screenshot" class="mb-2 rounded max-h-48" />
    {/if}
    <pre class="whitespace-pre-wrap font-sans">{m.content}</pre>
  </div>
</div>
