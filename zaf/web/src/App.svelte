<script>
  import { onMount } from 'svelte';
  import { fetchState, watchChanges } from './api.js';
  import MachineLine from './MachineLine.svelte';
  import NextForYou from './NextForYou.svelte';
  import ProjectSections from './ProjectSections.svelte';
  import { hasKey, takeKey } from './controls.js';

  // The link's key is read once, kept in memory and stripped from the address.
  takeKey();

  let state = $state(null);
  let refused = $state('');

  // Reads the state again; a refusal shows until the next read succeeds.
  async function read() {
    try {
      state = await fetchState();
      refused = '';
    } catch (error) {
      refused = error.message;
    }
  }

  onMount(() => {
    read();
    return watchChanges(read);
  });
</script>

<!-- The layout shell: the header holds the machine line; main holds Next for you first, then
     (a later task) one section for each project. -->
<header class="top">
  <h1>zaf hub</h1>
  {#if refused}
    <p class="refused" role="alert">{refused}</p>
  {:else if state}
    <MachineLine machine={state.machine} />
  {:else}
    <p class="machine-line">reading the machine…</p>
  {/if}
</header>

<main class="sections">
  {#if state}
    <NextForYou next={state.next} />
    <ProjectSections
      projects={state.projects ?? []}
      controls={state.controls === true && hasKey()}
    />
  {/if}
</main>
