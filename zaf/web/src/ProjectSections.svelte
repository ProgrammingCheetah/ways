<script>
  import ProjectSection from './ProjectSection.svelte';

  let { projects, controls = false } = $props();
</script>

{#each projects as project (project.root)}
  <ProjectSection {project} {controls} />
{/each}
