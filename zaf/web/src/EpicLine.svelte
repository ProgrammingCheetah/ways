<script module>
  // The epics whose finish has played in this tab, by project and number (two projects may share a number).
  const finished = new Set();
</script>

<script>
  // One epic's line: its own square beside its title, then the grid of its tasks' squares, coloured by state.
  // Each block is one focusable stop (a task's is its first square; its other squares are for the pointer
  // alone); hovering any square or focusing the stop shows a card that tells what it is.
  // It reads GET /api/state's `epic.tasks[]` (number, title, size, state, phase, blocked_by, question,
  // issue_url) and the epic's own `line_state` and `line_question`.
  import { safeUrl } from './api.js';
  import { gridCells, gridRows, turnedDone } from './line.js';
  import QuestionPrompt from './QuestionPrompt.svelte';

  // `controls`: the hub serves controls and this page holds its key; only then does a purple block open
  // the prompt to answer its question (`project` is the root the answer is sent for). `questions` is the
  // project's open questions, whose `url` is the question's issue.
  let { epic, project = null, controls = false, questions = [] } = $props();
  const uid = $props.id();

  // The six colours, each with the words that name it (the legend, and the accessible names).
  const STATES = [
    { key: 'done', word: 'done' },
    { key: 'question', word: 'has a question open' },
    { key: 'working', word: 'working' },
    { key: 'blocked', word: 'blocked' },
    { key: 'waiting_tick', word: 'waiting for the next tick' },
    { key: 'not_started', word: 'not started' },
  ];
  const known = new Set(STATES.map((s) => s.key));
  const wordOf = (state) => STATES.find((s) => s.key === state)?.word ?? 'not started';
  const classOf = (state) => (known.has(state) ? state : 'not_started');

  // The fill of each of a block's `count` squares, 0 to 1: a working task fills in proportion to its phases.
  function fills(count, state, done, total) {
    if (state !== 'working' || !(total > 0)) return Array(count).fill(1);
    const share = Math.min(Math.max(done / total, 0), 1) * count;
    return Array.from({ length: count }, (_, i) => Math.min(Math.max(share - i, 0), 1));
  }

  // The motion each state carries, as the class its squares take; blocked and not_started stay still.
  const MOTION = { question: 'bounce', waiting_tick: 'shimmer', done: 'sheen' };

  // Which squares of a block move: a working task pulses its next unfilled square (the last when all are
  // filled); every square of the other moving states carries its motion.
  function motions(state, squares) {
    if (state === 'working') {
      const next = squares.findIndex((f) => f < 1);
      const at = next === -1 ? squares.length - 1 : next;
      return squares.map((_, i) => (i === at ? 'pulse' : ''));
    }
    return squares.map(() => MOTION[state] ?? '');
  }

  // The tasks that turned done on the latest read shine once. The last read is kept in memory for the
  // tab only; the first read has nothing to compare with, so a task done then does not shine.
  let previous = null;
  let shining = $state([]);
  // The whole line's finish plays on the read where the epic's last task turns done, once for each epic
  // for the tab (kept in the module's memory, so a later read or a reopened epic plays none).
  let finishing = $state(false);
  const reduced = () => typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;

  const tasks = $derived(epic.tasks ?? []);
  $effect.pre(() => {
    const next = tasks;
    const { numbers, epic: whole } = turnedDone(previous, next);
    previous = next.map((t) => ({ number: t.number, state: t.state }));
    const still = reduced();
    shining = still ? [] : numbers;
    const key = `${project ?? ''}\n${epic.number}`;
    const play = whole && !still && !finished.has(key);
    if (play) finished.add(key);
    finishing = play;
  });
  const doneCount = $derived(tasks.filter((t) => t.state === 'done').length);
  // The grid: its rows, and the place of every task square (gridCells), a task's squares one after another.
  const cells = $derived(gridCells(tasks));
  const rows = $derived(gridRows(cells.length));
  const columns = $derived(cells.reduce((most, c) => Math.max(most, c.column + 1), 1));
  const blocks = $derived(
    tasks.map((t) => {
      const mine = cells.filter((c) => c.task === t);
      const phases = t.state === 'working' && t.phases_total > 0 ? ` (${t.phases_done} of ${t.phases_total} phases)` : '';
      const squares = fills(mine.length, t.state, t.phases_done, t.phases_total);
      return {
        task: t,
        cls: classOf(t.state),
        href: safeUrl(t.issue_url),
        cells: mine,
        first: cells.indexOf(mine[0]),
        squares,
        motion: motions(t.state, squares),
        shine: shining.includes(t.number),
        name: `Task #${t.number} ${t.title}: ${wordOf(t.state)}${phases}`,
      };
    }),
  );
  // The wave runs along the grid's squares in grid order (down a column and on to the next) and reaches
  // the epic's square last, in under two seconds however long it is.
  const step = $derived(Math.min(0.04, 1.2 / (cells.length + 1)));
  const epicClass = $derived(classOf(epic.line_state));
  const epicName = $derived(`Epic #${epic.number} ${epic.title}: ${wordOf(epic.line_state)}`);
  const epicHref = $derived(safeUrl(epic.url));

  // A purple task's prompt asks `epic.tasks[].question`; the epic's own asks `line_question`.
  // The prompt: the question asked, its issue's address and the block to give focus back to.
  let prompt = $state(null);

  const askable = (state, question) => controls && project !== null && state === 'question' && Boolean(question);

  // The question's issue: the project's own list knows it; failing that, the address of the ticket it is
  // raised on with its number swapped for the question's.
  function issueOf(question, ticketUrl, ticketNumber) {
    const listed = safeUrl(questions.find((q) => q.number === question.number)?.url);
    if (listed) return listed;
    const base = safeUrl(ticketUrl);
    if (!base || !base.endsWith(`/${ticketNumber}`)) return null;
    return safeUrl(`${base.slice(0, -String(ticketNumber).length)}${question.number}`);
  }

  function ask(question, href, event) {
    hide();
    prompt = { question, href, opener: event.currentTarget };
  }

  function closePrompt() {
    const opener = prompt?.opener;
    prompt = null;
    opener?.focus();
  }

  // The one card that shows: its key (`epic` or a task's number) and the block it hangs from.
  let active = $state(null);
  let anchor = $state(null);
  let card = $state(null);
  let place = $state({ left: 0, top: 0 });
  const cardId = (key) => `${uid}-card-${key}`;

  // The task the pointer is on (every one of its squares is outlined), apart from `active`, which focus sets too.
  let pointed = $state(null);

  // A block's card hangs from the square under the pointer, or from its first square when focus brings it.
  function show(key, event) {
    active = key;
    anchor = event.currentTarget.querySelector('.square') ?? event.currentTarget;
  }

  function point(key, event) {
    pointed = key;
    active = key;
    anchor = event.currentTarget;
  }

  function hide() {
    active = null;
    anchor = null;
    pointed = null;
  }

  // A task's later squares are not in the tab order: a click on one goes where its first square goes, the
  // same click with its keys (so a new tab still opens), and so the prompt returns focus to the first square.
  function clickFirst(event) {
    const first = event.currentTarget.closest('.task-block')?.querySelector('.hit');
    first?.dispatchEvent(
      new MouseEvent('click', {
        bubbles: true,
        cancelable: true,
        button: event.button,
        ctrlKey: event.ctrlKey,
        shiftKey: event.shiftKey,
        altKey: event.altKey,
        metaKey: event.metaKey,
      }),
    );
  }

  const squareClass = (block, i) =>
    ['square', block.cls, block.motion[i], block.squares[i] < 1 && 'partial', block.shine && 'shine', finishing && 'finish', pointed === block.task.number && 'outlined']
      .filter(Boolean)
      .join(' ');
  const squareStyle = (block, i) => {
    const cell = block.cells[i];
    return [`grid-row: ${cell.row + 1}`, `grid-column: ${cell.column + 1}`, block.shine ? `--i: ${i}` : '', finishing ? `--n: ${block.first + i}` : ''].filter(Boolean).join('; ');
  };

  // Keeps the card inside the viewport: under its block, or above it when there is no room below.
  $effect(() => {
    if (!card || !anchor) return;
    const margin = 4;
    const at = anchor.getBoundingClientRect();
    const width = card.offsetWidth;
    const height = card.offsetHeight;
    const left = Math.max(margin, Math.min(at.left, window.innerWidth - width - margin));
    const below = at.bottom + margin;
    const top = below + height > window.innerHeight && at.top - height - margin >= 0 ? at.top - height - margin : below;
    place = { left, top };
  });
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && hide()} onscroll={hide} onresize={hide} />

{#snippet question(q)}
  <p class="card-question" data-testid="card-question">{q.text}</p>
{/snippet}

{#if tasks.length}
  <div class="epic-line" data-testid="epic-line" data-epic={epic.number} style={finishing ? `--step: ${step}s` : null}>
    <div class="line-title">
      <ul class="epic-own">
      <li
        class="block epic-block"
        data-testid="epic-block"
        data-state={epicClass}
        onmouseenter={(e) => show('epic', e)}
        onmouseleave={hide}
        onfocusin={(e) => show('epic', e)}
        onfocusout={hide}
      >
        {#snippet epicSquares()}
          <span role="img" aria-label={epicName}>
            <span class="square {epicClass}" class:finish={finishing} style={finishing ? `--n: ${cells.length}` : null} data-fill="1"></span>
          </span>
        {/snippet}
        {#if askable(epic.line_state, epic.line_question)}
          <button
            type="button"
            class="hit"
            data-testid="ask-epic"
            aria-describedby={active === 'epic' ? cardId('epic') : undefined}
            onclick={(e) => ask(epic.line_question, issueOf(epic.line_question, epic.url, epic.number), e)}
            >{@render epicSquares()}</button
          >
        {:else if epicHref}
          <a class="hit" href={epicHref} aria-describedby={active === 'epic' ? cardId('epic') : undefined}>{@render epicSquares()}</a>
        {:else}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex: a block with no safe address is focusable, opens nothing -->
          <span class="hit" tabindex="0" aria-describedby={active === 'epic' ? cardId('epic') : undefined}>{@render epicSquares()}</span>
        {/if}
        {#if active === 'epic'}
          <div
            class="card"
            role="tooltip"
            id={cardId('epic')}
            data-testid="line-card"
            bind:this={card}
            style="left: {place.left}px; top: {place.top}px"
          >
            <p class="card-head"><span class="id">#{epic.number}</span> {epic.title}</p>
            <p class="card-state">{wordOf(epic.line_state)}</p>
            {#if epic.line_question}{@render question(epic.line_question)}{/if}
          </div>
        {/if}
      </li>
      </ul>
      <span class="id">#{epic.number}</span> <span class="title">{epic.title}</span>
    </div>
    <ul class="line">
      <!-- The box scrolls on its own, and an element's scroll never reaches the window: it hides the card too. -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <li class="grid-box" data-testid="epic-grid-box" onscroll={hide}>
      <ul class="grid" data-testid="epic-grid" data-rows={rows} style="--rows: {rows}; --cols: {columns}">
      {#each blocks as block (block.task.number)}
        {@const t = block.task}
        <li
          class="block task-block"
          data-testid="task-block"
          data-task={t.number}
          data-state={block.cls}
          onfocusin={(e) => show(t.number, e)}
          onfocusout={hide}
        >
          {#snippet inside(fill)}
            {#if block.cls === 'working'}
              <span class="fill" style="transform: scaleX({fill})"></span>
            {/if}
          {/snippet}
          {#each block.squares as fill, i (i)}
            {@const cell = block.cells[i]}
            {#if i === 0}
              <!-- A task's first square is its one stop: the link to its issue, or the button that opens its prompt,
                   with the task's name. -->
              {#if askable(t.state, t.question)}
                <button
                  type="button"
                  class="hit {squareClass(block, i)}"
                  style={squareStyle(block, i)}
                  data-testid="ask-task"
                  data-row={cell.row + 1}
                  data-column={cell.column + 1}
                  data-fill={fill}
                  aria-describedby={active === t.number ? cardId(t.number) : undefined}
                  onmouseenter={(e) => point(t.number, e)}
                  onmouseleave={hide}
                  onclick={(e) => ask(t.question, issueOf(t.question, t.issue_url, t.number), e)}
                  ><span role="img" aria-label={block.name}></span>{@render inside(fill)}</button
                >
              {:else if block.href}
                <a
                  class="hit {squareClass(block, i)}"
                  style={squareStyle(block, i)}
                  href={block.href}
                  data-row={cell.row + 1}
                  data-column={cell.column + 1}
                  data-fill={fill}
                  aria-describedby={active === t.number ? cardId(t.number) : undefined}
                  onmouseenter={(e) => point(t.number, e)}
                  onmouseleave={hide}
                  ><span role="img" aria-label={block.name}></span>{@render inside(fill)}</a
                >
              {:else}
                <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_static_element_interactions, a11y_mouse_events_have_key_events: a block with no safe address is focusable, opens nothing -->
                <span
                  class="hit {squareClass(block, i)}"
                  style={squareStyle(block, i)}
                  tabindex="0"
                  data-row={cell.row + 1}
                  data-column={cell.column + 1}
                  data-fill={fill}
                  aria-describedby={active === t.number ? cardId(t.number) : undefined}
                  onmouseenter={(e) => point(t.number, e)}
                  onmouseleave={hide}
                  ><span role="img" aria-label={block.name}></span>{@render inside(fill)}</span
                >
              {/if}
            {:else}
              <!-- The later squares: out of the tab order and the reading, they show the card, outline the task and
                   click through to the first square's place. -->
              <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events, a11y_mouse_events_have_key_events -->
              <span
                class={squareClass(block, i)}
                class:clickable={askable(t.state, t.question) || block.href}
                style={squareStyle(block, i)}
                aria-hidden="true"
                data-row={cell.row + 1}
                data-column={cell.column + 1}
                data-fill={fill}
                onmouseenter={(e) => point(t.number, e)}
                onmouseleave={hide}
                onclick={clickFirst}
                >{@render inside(fill)}</span
              >
            {/if}
          {/each}
          {#if active === t.number}
            <div
              class="card"
              role="tooltip"
              id={cardId(t.number)}
              data-testid="line-card"
              bind:this={card}
              style="left: {place.left}px; top: {place.top}px"
            >
              <p class="card-head"><span class="id">#{t.number}</span> {t.title}</p>
              <p class="card-meta">
                {#if t.size}<span data-testid="card-size">size {t.size}</span>{/if}
                <span data-testid="card-state">{wordOf(t.state)}</span>
                {#if t.phase}<span data-testid="card-phase">phase {t.phase}</span>{/if}
              </p>
              {#if t.blocked_by?.length}
                <ul class="card-blockers" data-testid="card-blockers">
                  {#each t.blocked_by as b (b.number)}
                    <li><span class="id">#{b.number}</span>{b.title ? ` ${b.title}` : ''}: {wordOf(b.state)}</li>
                  {/each}
                </ul>
              {/if}
              {#if t.question}{@render question(t.question)}{/if}
            </div>
          {/if}
        </li>
      {/each}
      </ul>
      </li>
    </ul>
    {#if prompt}
      {#key prompt.question.number}
        <QuestionPrompt {project} question={prompt.question} href={prompt.href} onclose={closePrompt} />
      {/key}
    {/if}
    <p class="line-count" data-testid="epic-line-count">{doneCount} of {tasks.length} done</p>
    <ul class="legend" data-testid="epic-line-legend" aria-label="Colours of the line">
      {#each STATES as state (state.key)}
        <li><span class="square swatch {state.key}" aria-hidden="true"></span>{state.word}</li>
      {/each}
    </ul>
  </div>
{/if}
