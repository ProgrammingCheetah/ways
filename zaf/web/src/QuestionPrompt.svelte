<script>
  // The prompt a yellow block on the line opens: the question, a field for the answer, a Send button
  // and a link to the question's issue. A native modal dialog: focus moves in, Escape and Close shut it.
  import Action from './Action.svelte';
  import { answer } from './controls.js';

  let { project, question, href = null, onclose } = $props();
  let dialog = $state(null);
  let draft = $state('');

  // Opens as a modal when the browser can; where it cannot (jsdom), the open attribute shows it.
  $effect(() => {
    if (!dialog || dialog.open) return;
    if (typeof dialog.showModal === 'function') dialog.showModal();
    else dialog.setAttribute('open', '');
    dialog.querySelector('textarea')?.focus();
  });

  function close() {
    if (typeof dialog?.close === 'function' && dialog.open) dialog.close();
    onclose();
  }

  function keydown(event) {
    if (event.key !== 'Escape') return;
    event.preventDefault();
    event.stopPropagation();
    close();
  }
</script>

<dialog
  class="prompt"
  data-testid="question-prompt"
  aria-labelledby="prompt-title-{question.number}"
  bind:this={dialog}
  onkeydown={keydown}
  oncancel={(e) => {
    e.preventDefault();
    close();
  }}
>
  <h3 id="prompt-title-{question.number}">Question #{question.number}</h3>
  <p class="prompt-question" data-testid="prompt-question">{question.text}</p>
  {#if href}
    <p><a href={href} rel="noreferrer" data-testid="prompt-issue">the question's issue</a></p>
  {/if}
  <label class="ruling">
    Your answer
    <textarea rows="4" bind:value={draft} data-testid="prompt-text"></textarea>
  </label>
  <div class="prompt-buttons">
    <Action label="Send" testid="prompt-send" act={() => answer(project, question.number, draft)} />
    <button type="button" class="control" data-testid="prompt-close" onclick={close}>Close</button>
  </div>
</dialog>
