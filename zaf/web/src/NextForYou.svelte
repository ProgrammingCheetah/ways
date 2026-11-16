<script>
  import { kindLabel, blocksLine, copyText } from './api.js';

  let { next } = $props();
  // The command last copied, to say so on its button for a moment.
  let copied = $state(null);

  async function copy(command) {
    if (await copyText(command)) {
      copied = command;
      setTimeout(() => {
        if (copied === command) copied = null;
      }, 1500);
    }
  }
</script>

<section class="next" aria-labelledby="next-heading">
  <h2 id="next-heading">Next for you</h2>
  {#if next.length === 0}
    <p class="nothing" data-testid="nothing-waiting">Nothing is waiting on you.</p>
  {:else}
    <ol class="next-list">
      {#each next as item, index (index)}
        <li class="next-item kind-{item.kind}" data-testid="next-item" data-kind={item.kind}>
          <p class="next-head">
            <span class="kind">{kindLabel(item.kind)}</span>
            {#if item.kind === 'door'}
              <span class="door" data-testid="door-mark" title="A door: a one-way decision"
                >door</span
              >
            {/if}
            <span class="project">{item.project}</span>
            <span class="blocks">{blocksLine(item.blocks)}</span>
          </p>
          <p class="title">{item.title}</p>
          <p class="how">{item.how}</p>
          {#if item.command}
            <div class="command">
              <code data-testid="next-command">{item.command}</code>
              <button type="button" class="copy" onclick={() => copy(item.command)}>
                {copied === item.command ? 'Copied' : 'Copy'}
              </button>
            </div>
          {/if}
          {#if item.link}
            <p class="link"><a href={item.link} target="_blank" rel="noreferrer">Open it</a></p>
          {/if}
        </li>
      {/each}
    </ol>
  {/if}
</section>
