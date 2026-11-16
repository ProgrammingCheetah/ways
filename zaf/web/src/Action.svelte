<script>
  // One control button. `act` does the work and returns what happened in words; a refusal is
  // thrown with its reason, and shows here instead.
  let { label, act, testid } = $props();
  let busy = $state(false);
  let said = $state('');
  let refusal = $state('');

  async function run() {
    busy = true;
    said = '';
    refusal = '';
    try {
      said = await act();
    } catch (error) {
      refusal = error.message;
    } finally {
      busy = false;
    }
  }
</script>

<span class="action">
  <button type="button" class="control" data-testid={testid} disabled={busy} onclick={run}>
    {label}
  </button>
  {#if said}<span class="said" role="status">{said}</span>{/if}
  {#if refusal}<span class="refusal" role="alert">{refusal}</span>{/if}
</span>
