<script>
  import Action from './Action.svelte';
  import { answer, proposalOf } from './controls.js';

  // A question to rule on: accept what it proposes, or edit the answer and send that.
  let { project, question } = $props();

  const proposal = $derived(proposalOf(question.text));
  let editing = $state(false);
  let draft = $state('');

  function edit() {
    draft = proposal ?? '';
    editing = true;
  }
</script>

<div class="answer" data-testid="answer-control">
  {#if proposal}
    <p class="proposal" data-testid="proposal">It proposes: {proposal}</p>
    <Action
      label="Accept"
      testid="answer-accept"
      act={() => answer(project, question.number, proposal)}
    />
  {:else}
    <p class="proposal">It proposes no answer; give yours.</p>
  {/if}
  {#if editing || !proposal}
    <label class="ruling">
      Your answer
      <textarea rows="3" bind:value={draft} data-testid="answer-text"></textarea>
    </label>
    <Action
      label="Send my answer"
      testid="answer-send"
      act={() => answer(project, question.number, draft)}
    />
  {:else}
    <button type="button" class="control" data-testid="answer-edit" onclick={edit}>Edit</button>
  {/if}
</div>
