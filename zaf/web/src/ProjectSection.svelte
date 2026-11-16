<script>
  import { ago, dollars, runTime, safeUrl, trouble } from './api.js';
  import { dropProject, startWatch, stopTask, stopWatch, tick as runTick } from './controls.js';
  import Action from './Action.svelte';
  import AnswerControl from './AnswerControl.svelte';

  // `controls`: the hub serves controls and this page holds its key; without both, no button shows.
  let { project, controls = false } = $props();

  const problem = $derived(trouble(project));
  const epics = $derived(project.epics ?? []);
  const queue = $derived(project.queue ?? []);
  const questions = $derived(project.questions ?? []);
  const instances = $derived(project.instances ?? []);
  const watch = $derived(project.watch ?? null);
  const tick = $derived(project.last_tick ?? null);
  const tickAt = $derived(tick?.at ?? watch?.last_tick ?? null);
  const started = $derived(tick?.started ?? []);
  const skipped = $derived(tick?.skipped ?? []);
</script>

<section class="project" data-testid="project-section" aria-label={project.project}>
  <h2>{project.project}</h2>

  {#if problem}
    <p class="note {project.missing ? 'missing' : 'unreachable'}" data-testid="project-trouble">
      {problem}
    </p>
    {#if controls && project.missing}
      <Action label="Drop from the list" testid="drop" act={() => dropProject(project.root)} />
    {/if}
  {/if}

  {#if !project.missing}
    <h3>Epics</h3>
    {#if epics.length}
      <ul class="rows" data-testid="epics">
        {#each epics as epic (epic.number)}
          <li>
            <div class="row-head">
              <span class="id">#{epic.number}</span>
              <span class="title">{epic.title}</span>
            </div>
            <div class="row-meta">
              <span class="badge">{epic.stage}</span>
              <span>{epic.done} of {epic.total} done</span>
              <span>{dollars(epic.dollars)}</span>
            </div>
            {#if epic.groups?.length}
              <ul class="groups">
                {#each epic.groups as group (group.name)}
                  <li>{group.name}: {group.done} of {group.total}</li>
                {/each}
              </ul>
            {/if}
          </li>
        {/each}
      </ul>
    {:else}
      <p class="empty">no open epics</p>
    {/if}

    <h3>Queue</h3>
    {#if queue.length}
      <ul class="rows" data-testid="queue">
        {#each queue as task (task.number)}
          <li>
            <div class="row-head">
              <span class="id">#{task.number}</span>
              <span class="title">{task.title}</span>
            </div>
            <div class="row-meta">
              <span class="badge state-{task.state}">{task.state}</span>
              <span>epic #{task.epic}</span>
              {#if task.phase}<span>{task.phase}</span>{/if}
              {#if safeUrl(task.issue_url)}
                <a href={safeUrl(task.issue_url)} rel="noreferrer">issue</a>
              {/if}
              {#if safeUrl(task.pull_request_url)}
                <a href={safeUrl(task.pull_request_url)} rel="noreferrer">pull request #{task.pull_request}</a>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    {:else}
      <p class="empty">the queue is empty</p>
    {/if}

    {#if questions.length}
      <h3>Questions</h3>
      <ul class="rows" data-testid="questions">
        {#each questions as question (question.number)}
          <li>
            <div class="row-head">
              <span class="id">#{question.number}</span>
              {#if question.door}<span class="badge">door</span>{/if}
              <span class="title">{question.text.split('\n').find((line) => line.trim()) ?? ''}</span>
            </div>
            {#if controls}
              <AnswerControl project={project.root} {question} />
            {/if}
          </li>
        {/each}
      </ul>
    {/if}

    <h3>Running</h3>
    {#if instances.length}
      <ul class="rows" data-testid="instances">
        {#each instances as instance (instance.instance)}
          <li>
            <div class="row-head">
              <span class="badge">{instance.role}</span>
              {#if instance.number !== null && instance.number !== undefined}
                <span class="id">#{instance.number}</span>
              {/if}
              <span class="title">{instance.title ?? instance.instance}</span>
            </div>
            <div class="row-meta">
              {#if instance.phase}<span>{instance.phase}</span>{/if}
              <span>running {runTime(instance.run_seconds)}</span>
              {#if instance.last_tool_call}<span>last: {instance.last_tool_call}</span>{/if}
            </div>
            {#if controls && instance.task !== null && instance.task !== undefined}
              <Action label="Stop" testid="stop" act={() => stopTask(project.root, instance.task)} />
            {/if}
          </li>
        {/each}
      </ul>
    {:else}
      <p class="empty">nothing is running</p>
    {/if}

    <h3>Ticks</h3>
    <div class="row-meta" data-testid="ticks">
      {#if watch}
        <span class="badge">{watch.running ? 'watch running' : 'watch not running'}</span>
      {/if}
      <span>{tickAt ? `last tick ${ago(tickAt)}` : 'no tick yet'}</span>
      {#if started.length}<span>started: {started.join(', ')}</span>{/if}
      {#if skipped.length}<span>skipped: {skipped.join('; ')}</span>{/if}
    </div>
    {#if controls}
      <div class="controls">
        <Action label="Tick once" testid="tick" act={() => runTick(project.root, project.project)} />
        {#if watch?.running}
          <Action label="Stop the watch" testid="watch" act={() => stopWatch(project.root)} />
        {:else}
          <Action
            label="Start the watch"
            testid="watch"
            act={() => startWatch(project.root, project.project, watch?.version ?? undefined)}
          />
        {/if}
      </div>
    {/if}
  {/if}
</section>
