import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import { forgetKey, takeKey } from './controls.js';
import EpicLine from './EpicLine.svelte';
import { turnedDone } from './line.js';

const task = (number, state, size, extra = {}) => ({
  number,
  title: `task ${number}`,
  size,
  state,
  phase: null,
  phases_done: 0,
  phases_total: 0,
  ...extra,
});

const epic = (tasks, extra = {}) => ({ number: 7, title: 'an epic', line_state: 'working', tasks, ...extra });

const squares = (el) => el.querySelectorAll('.square');

describe('epic line', () => {
  it('epic line draws a run of squares per task by size', () => {
    render(EpicLine, { epic: epic([task(1, 'done', 'XS'), task(2, 'working', 'M'), task(3, 'not_started', null)]) });
    const own = screen.getByTestId('epic-block');
    const blocks = screen.getAllByTestId('task-block');
    expect(squares(own)).toHaveLength(2);
    // The epic's own block comes before the first task.
    expect(own.compareDocumentPosition(blocks[0]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(blocks.map((b) => b.dataset.task)).toEqual(['1', '2', '3']);
    expect(blocks.map((b) => squares(b).length)).toEqual([1, 3, 2]);
  });

  it('epic line draws the sizes S, L, XL and E', () => {
    render(EpicLine, {
      epic: epic([task(1, 'done', 'S'), task(2, 'done', 'L'), task(3, 'done', 'XL'), task(4, 'done', 'E')]),
    });
    expect(screen.getAllByTestId('task-block').map((b) => squares(b).length)).toEqual([2, 4, 5, 6]);
  });

  it('epic line colours each state', () => {
    const states = ['done', 'question', 'working', 'blocked', 'waiting_tick', 'not_started'];
    render(EpicLine, { epic: epic(states.map((s, i) => task(i + 1, s, 'XS')), { line_state: 'question' }) });
    const blocks = screen.getAllByTestId('task-block');
    states.forEach((s, i) => expect(squares(blocks[i])[0].classList.contains(s)).toBe(true));
    // The epic's own block takes its line_state.
    expect(squares(screen.getByTestId('epic-block'))[0].classList.contains('question')).toBe(true);
    const legend = screen.getByTestId('epic-line-legend').textContent;
    for (const word of ['done', 'question', 'working', 'blocked', 'waiting for the next tick', 'not started']) {
      expect(legend).toContain(word);
    }
  });

  it('epic line names each block in words', () => {
    render(EpicLine, { epic: epic([task(5, 'blocked', 'S', { title: 'wait on it' })], { line_state: 'done' }) });
    expect(screen.getByRole('img', { name: 'Task #5 wait on it: blocked' })).toBeTruthy();
    expect(screen.getByRole('img', { name: 'Epic #7 an epic: done' })).toBeTruthy();
  });

  it('epic line counts tasks done', () => {
    render(EpicLine, { epic: epic([task(1, 'done', 'S'), task(2, 'working', 'S'), task(3, 'blocked', 'S')]) });
    expect(screen.getByTestId('epic-line-count').textContent).toBe('1 of 3 done');
  });

  it('epic line fills a working task by phases', () => {
    render(EpicLine, { epic: epic([task(1, 'working', 'M', { phases_done: 1, phases_total: 3 })]) });
    const own = within(screen.getByTestId('task-block')).getByRole('img');
    const [a, b, c] = squares(own);
    expect([a, b, c].map((s) => Number(s.dataset.fill))).toEqual([1, 0, 0]);
    expect(a.classList.contains('partial')).toBe(false);
    expect(b.classList.contains('partial')).toBe(true);
  });

  it('epic line draws no line for an epic with no tasks', () => {
    render(EpicLine, { epic: epic([]) });
    expect(screen.queryByTestId('epic-line')).toBeNull();
  });
});

describe('line card', () => {
  const url = (n) => `https://example.test/issues/${n}`;

  it('line card shows a task on hover', async () => {
    render(EpicLine, {
      epic: epic([task(5, 'working', 'M', { title: 'build it', phase: 'implement', issue_url: url(5) })]),
    });
    expect(screen.queryByTestId('line-card')).toBeNull();
    const block = screen.getByTestId('task-block');
    await fireEvent.mouseEnter(block);
    const card = screen.getByTestId('line-card');
    expect(card.textContent).toContain('#5');
    expect(card.textContent).toContain('build it');
    expect(card.textContent).toContain('size M');
    expect(card.textContent).toContain('working');
    expect(card.textContent).toContain('phase implement');
    // The card is tied to its block, and closes when the mouse leaves.
    expect(within(block).getByRole('link').getAttribute('aria-describedby')).toBe(card.id);
    await fireEvent.mouseLeave(block);
    expect(screen.queryByTestId('line-card')).toBeNull();
  });

  it('line card shows on keyboard focus', async () => {
    render(EpicLine, { epic: epic([task(1, 'done', 'S', { issue_url: url(1) }), task(2, 'blocked', 'S')]) });
    const links = screen.getAllByRole('link');
    links[0].focus();
    await tick();
    expect(screen.getByTestId('line-card').textContent).toContain('done');
    // Escape closes it.
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(screen.queryByTestId('line-card')).toBeNull();
    // A task with no address is still a stop on the line, and blur closes its card.
    const stop = screen.getAllByTestId('task-block')[1].querySelector('.hit');
    expect(stop.getAttribute('tabindex')).toBe('0');
    stop.focus();
    await tick();
    expect(screen.getByTestId('line-card').textContent).toContain('blocked');
    stop.blur();
    await tick();
    expect(screen.queryByTestId('line-card')).toBeNull();
  });

  it('line card lists a blocked task\'s blockers', async () => {
    const blocked_by = [
      { number: 3, title: 'first part', state: 'working' },
      { number: 4, title: null, state: 'not_started' },
    ];
    render(EpicLine, { epic: epic([task(9, 'blocked', 'S', { blocked_by })]) });
    await fireEvent.mouseEnter(screen.getByTestId('task-block'));
    const items = within(screen.getByTestId('card-blockers')).getAllByRole('listitem');
    expect(items.map((li) => li.textContent.trim())).toEqual(['#3 first part: working', '#4: not started']);
  });

  it('line card shows a yellow task\'s question, as text', async () => {
    const question = { number: 20, text: 'Which <b>door</b>?' };
    render(EpicLine, { epic: epic([task(8, 'question', 'S', { question })]) });
    await fireEvent.mouseEnter(screen.getByTestId('task-block'));
    const text = screen.getByTestId('card-question');
    expect(text.textContent).toBe('Which <b>door</b>?');
    expect(text.querySelector('b')).toBeNull();
  });

  it('line card tells the epic\'s own state and its question', async () => {
    render(EpicLine, {
      epic: epic([task(1, 'done', 'S')], { line_state: 'question', line_question: { number: 30, text: 'Sign it?' } }),
    });
    await fireEvent.mouseEnter(screen.getByTestId('epic-block'));
    const card = screen.getByTestId('line-card');
    expect(card.textContent).toContain('#7');
    expect(card.textContent).toContain('an epic');
    expect(card.textContent).toContain('has a question open');
    expect(screen.getByTestId('card-question').textContent).toBe('Sign it?');
  });

  it('line card tells a working epic with no question', async () => {
    render(EpicLine, { epic: epic([task(1, 'done', 'S')]) });
    await fireEvent.mouseEnter(screen.getByTestId('epic-block'));
    expect(screen.getByTestId('line-card').textContent).toContain('working');
    expect(screen.queryByTestId('card-question')).toBeNull();
  });

  it('line task links only to a safe issue url', () => {
    render(EpicLine, {
      epic: epic([
        task(1, 'done', 'S', { issue_url: url(1) }),
        task(2, 'done', 'S', { issue_url: 'javascript:alert(1)' }),
        task(3, 'done', 'S', { issue_url: null }),
      ]),
    });
    const blocks = screen.getAllByTestId('task-block');
    expect(within(blocks[0]).getByRole('link').getAttribute('href')).toBe(url(1));
    expect(within(blocks[1]).queryByRole('link')).toBeNull();
    expect(within(blocks[2]).queryByRole('link')).toBeNull();
  });
});

describe('line motion', () => {
  const MOTIONS = ['pulse', 'bounce', 'shimmer', 'sheen'];
  const motionsOf = (el) => MOTIONS.filter((m) => el.classList.contains(m));

  it('line motion pulses the square being worked on', () => {
    render(EpicLine, { epic: epic([task(1, 'working', 'M', { phases_done: 1, phases_total: 3 })]) });
    const [a, b, c] = squares(screen.getByTestId('task-block'));
    expect([a, b, c].map(motionsOf)).toEqual([[], ['pulse'], []]);
  });

  it('line motion class by state', () => {
    const states = ['working', 'question', 'waiting_tick', 'done', 'blocked', 'not_started'];
    render(EpicLine, { epic: epic(states.map((s, i) => task(i + 1, s, 'S', { phases_done: 0, phases_total: 2 }))) });
    const blocks = screen.getAllByTestId('task-block');
    const seen = blocks.map((b) => [...new Set([...squares(b)].flatMap((s) => motionsOf(s)))]);
    expect(seen).toEqual([['pulse'], ['bounce'], ['shimmer'], ['sheen'], [], []]);
    // The epic's own block and the legend stay still.
    expect(motionsOf(squares(screen.getByTestId('epic-block'))[0])).toEqual([]);
    for (const s of squares(screen.getByTestId('epic-line-legend'))) expect(motionsOf(s)).toEqual([]);
  });

  it('line motion fills as phases pass', async () => {
    const { rerender } = render(EpicLine, {
      epic: epic([task(1, 'working', 'M', { phases_done: 1, phases_total: 3 })]),
    });
    const second = squares(screen.getByTestId('task-block'))[1];
    expect(second.querySelector('.fill').style.transform).toBe('scaleX(0)');
    await rerender({ epic: epic([task(1, 'working', 'M', { phases_done: 2, phases_total: 3 })]) });
    const after = squares(screen.getByTestId('task-block'));
    // The same elements are updated in place, so the fill eases and the page is not rebuilt.
    expect(after[1]).toBe(second);
    expect([...after].map((s) => Number(s.dataset.fill))).toEqual([1, 1, 0]);
    expect(second.querySelector('.fill').style.transform).toBe('scaleX(1)');
    expect(motionsOf(after[2])).toEqual(['pulse']);
    expect(motionsOf(second)).toEqual([]);
  });

  it('line motion stops under reduced motion', () => {
    const css = readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), 'app.css'), 'utf8');
    const block = (query) => {
      const start = css.indexOf(`@media (${query})`);
      expect(start).toBeGreaterThan(-1);
      let depth = 0;
      for (let i = css.indexOf('{', start); i < css.length; i++) {
        if (css[i] === '{') depth++;
        if (css[i] === '}' && --depth === 0) return [start, i];
      }
      throw new Error('unclosed media block');
    };
    const [rs, re] = block('prefers-reduced-motion: reduce');
    const reduce = css.slice(rs, re);
    expect(reduce).toMatch(/animation:\s*none/);
    expect(reduce).toMatch(/transition:\s*none/);
    // Every running animation and transition sits inside the no-preference block.
    const [from, to] = block('prefers-reduced-motion: no-preference');
    const running = [...css.matchAll(/^\s*(animation|transition):\s*(?!\s|none)[^;]+;/gm)];
    expect(running.length).toBeGreaterThan(0);
    for (const m of running) expect(m.index > from && m.index < to).toBe(true);
  });
});

describe('line shine', () => {
  const shone = (el) => [...squares(el)].filter((s) => s.classList.contains('shine')).length;
  const asReduced = (reduce) => {
    globalThis.matchMedia = (q) => ({ matches: reduce && q.includes('reduce'), media: q });
  };
  afterEach(() => {
    delete globalThis.matchMedia;
  });

  it('line shine plays once when a task turns done', async () => {
    asReduced(false);
    const { rerender } = render(EpicLine, { epic: epic([task(1, 'claimed', 'M'), task(2, 'working', 'S')]) });
    expect(shone(screen.getAllByTestId('task-block')[0])).toBe(0);
    await rerender({ epic: epic([task(1, 'done', 'M'), task(2, 'working', 'S')]) });
    const [first, second] = screen.getAllByTestId('task-block');
    expect(shone(first)).toBe(3);
    expect(shone(second)).toBe(0);
    // Each square starts a beat after the one before.
    expect([...squares(first)].map((s) => s.style.getPropertyValue('--i'))).toEqual(['0', '1', '2']);
  });

  it('line shine skips a task done at first read', () => {
    asReduced(false);
    render(EpicLine, { epic: epic([task(1, 'done', 'M')]) });
    expect(shone(screen.getByTestId('task-block'))).toBe(0);
  });

  it('line shine does not repeat', async () => {
    asReduced(false);
    const { rerender } = render(EpicLine, { epic: epic([task(1, 'working', 'S'), task(2, 'working', 'S')]) });
    await rerender({ epic: epic([task(1, 'done', 'S'), task(2, 'working', 'S')]) });
    expect(shone(screen.getAllByTestId('task-block')[0])).toBe(2);
    await rerender({ epic: epic([task(1, 'done', 'S'), task(2, 'working', 'S', { phases_done: 1, phases_total: 2 })]) });
    expect(shone(screen.getAllByTestId('task-block')[0])).toBe(0);
    await rerender({ epic: epic([task(1, 'done', 'S'), task(2, 'done', 'S')]) });
    const [first, second] = screen.getAllByTestId('task-block');
    expect([shone(first), shone(second)]).toEqual([0, 2]);
  });

  it('line shine stops under reduced motion', async () => {
    asReduced(true);
    const { rerender } = render(EpicLine, { epic: epic([task(1, 'claimed', 'S')]) });
    await rerender({ epic: epic([task(1, 'done', 'S')]) });
    expect(shone(screen.getByTestId('task-block'))).toBe(0);
    // The stylesheet runs the shine only when motion is not reduced.
    const css = readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), 'app.css'), 'utf8');
    const start = css.indexOf('@media (prefers-reduced-motion: no-preference)');
    const end = css.indexOf('@media (prefers-reduced-motion: reduce)');
    expect(css.slice(start, end)).toMatch(/\.square\.shine::before\s*\{[^}]*animation:\s*square-shine/);
    expect(css.slice(end)).toMatch(/\.square::before/);
  });

  it('turned done reports the whole epic once', () => {
    const a = (...s) => s.map((state, i) => ({ number: i + 1, state }));
    expect(turnedDone(null, a('done', 'done'))).toEqual({ numbers: [], epic: false });
    expect(turnedDone(a('working', 'done'), a('done', 'done'))).toEqual({ numbers: [1], epic: true });
    expect(turnedDone(a('working', 'working'), a('done', 'working'))).toEqual({ numbers: [1], epic: false });
    expect(turnedDone(a('working', 'working'), a('done', 'done'))).toEqual({ numbers: [1, 2], epic: true });
    expect(turnedDone(a('done', 'done'), a('done', 'done'))).toEqual({ numbers: [], epic: false });
  });
});

describe('line prompt', () => {
  const KEY = 'k3y-0123456789';
  const ROOT = '/home/zuri/code/ai/ways';
  const question = { number: 20, text: 'Which door?' };
  const asked = (extra = {}) =>
    epic([task(8, 'question', 'S', { question, issue_url: 'https://example.test/issues/8' })], extra);

  // The hub answers a POST with `reply`; the posts are kept.
  function hub(reply = { status: 200, body: { answer: '#20 ruled' } }) {
    const posts = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url, init) => {
        posts.push({ url, init, body: JSON.parse(init.body) });
        return { ok: reply.status < 400, status: reply.status, json: async () => reply.body };
      }),
    );
    return posts;
  }

  const withKey = () => {
    window.history.replaceState(null, '', `/#key=${KEY}`);
    takeKey();
  };

  beforeEach(() => forgetKey());
  afterEach(() => {
    vi.unstubAllGlobals();
    window.history.replaceState(null, '', '/');
  });

  it('line prompt opens on a yellow task with the key', async () => {
    withKey();
    render(EpicLine, { epic: asked(), project: ROOT, controls: true, questions: [] });
    expect(screen.queryByTestId('question-prompt')).toBeNull();
    const block = screen.getByTestId('task-block');
    await fireEvent.click(within(block).getByTestId('ask-task'));
    const prompt = screen.getByTestId('question-prompt');
    expect(within(prompt).getByTestId('prompt-question').textContent).toBe('Which door?');
    expect(within(prompt).getByTestId('prompt-issue').getAttribute('href')).toBe('https://example.test/issues/20');
    expect(within(prompt).getByTestId('prompt-text')).toBeTruthy();
  });

  it('line prompt opens on the epic when it is yellow', async () => {
    withKey();
    render(EpicLine, {
      epic: epic([task(1, 'done', 'S')], {
        line_state: 'question',
        line_question: { number: 30, text: 'Sign it?' },
        url: 'https://example.test/issues/7',
      }),
      project: ROOT,
      controls: true,
    });
    await fireEvent.click(screen.getByTestId('ask-epic'));
    expect(screen.getByTestId('prompt-question').textContent).toBe('Sign it?');
    expect(screen.getByTestId('prompt-issue').getAttribute('href')).toBe('https://example.test/issues/30');
  });

  it('line prompt sends the answer', async () => {
    withKey();
    const posts = hub();
    render(EpicLine, { epic: asked(), project: ROOT, controls: true });
    await fireEvent.click(screen.getByTestId('ask-task'));
    await fireEvent.input(screen.getByTestId('prompt-text'), { target: { value: 'The left one.' } });
    await fireEvent.click(screen.getByTestId('prompt-send'));
    expect(await screen.findByText('#20 ruled')).toBeTruthy();
    expect(posts).toHaveLength(1);
    expect(posts[0].url).toBe('/api/answer');
    expect(posts[0].init.headers['X-Zaf-Key']).toBe(KEY);
    expect(posts[0].body).toEqual({ project: ROOT, issue: 20, ruling: 'The left one.' });
  });

  it('line prompt shows a refusal', async () => {
    withKey();
    hub({ status: 409, body: { refused: 'that question is already ruled' } });
    render(EpicLine, { epic: asked(), project: ROOT, controls: true });
    await fireEvent.click(screen.getByTestId('ask-task'));
    await fireEvent.click(screen.getByTestId('prompt-send'));
    const alert = await within(screen.getByTestId('question-prompt')).findByRole('alert');
    expect(alert.textContent).toBe('that question is already ruled');
  });

  it('line prompt closes on escape', async () => {
    withKey();
    render(EpicLine, { epic: asked(), project: ROOT, controls: true });
    const opener = screen.getByTestId('ask-task');
    opener.focus();
    await fireEvent.click(opener);
    await fireEvent.keyDown(screen.getByTestId('question-prompt'), { key: 'Escape' });
    await waitFor(() => expect(screen.queryByTestId('question-prompt')).toBeNull());
    expect(document.activeElement).toBe(opener);
    // The Close button shuts it too.
    await fireEvent.click(opener);
    await fireEvent.click(screen.getByTestId('prompt-close'));
    expect(screen.queryByTestId('question-prompt')).toBeNull();
    expect(document.activeElement).toBe(opener);
  });

  it('line yellow task opens its issue without the key', async () => {
    // Controls false: the block is the issue's link.
    const { unmount } = render(EpicLine, { epic: asked(), project: ROOT, controls: false });
    expect(screen.queryByTestId('ask-task')).toBeNull();
    expect(within(screen.getByTestId('task-block')).getByRole('link').getAttribute('href')).toBe(
      'https://example.test/issues/8',
    );
    unmount();
    // Nothing given: the same.
    render(EpicLine, { epic: asked(), project: ROOT });
    expect(screen.queryByTestId('ask-task')).toBeNull();
    expect(within(screen.getByTestId('task-block')).getByRole('link')).toBeTruthy();
  });
});

describe('line finish', () => {
  const finished = (root) => [...squares(root)].filter((s) => s.classList.contains('finish')).length;
  const asReduced = (reduce) => {
    globalThis.matchMedia = (q) => ({ matches: reduce && q.includes('reduce'), media: q });
  };
  // Each test its own epic number: the finish is remembered by epic for the tab.
  const open = (n, ...states) => epic(states.map((s, i) => task(i + 1, s, 'S')), { number: n });
  afterEach(() => {
    delete globalThis.matchMedia;
  });

  it('line finish plays once when the last task turns done', async () => {
    asReduced(false);
    const { rerender } = render(EpicLine, { epic: open(101, 'done', 'working') });
    expect(finished(screen.getByTestId('epic-line'))).toBe(0);
    await rerender({ epic: open(101, 'done', 'done') });
    const line = screen.getByTestId('epic-line');
    // The epic's two squares and each task's two, in a wave that starts at the epic's block.
    expect(finished(line)).toBe(6);
    const n = [...squares(line.querySelector('.line'))].map((s) => s.style.getPropertyValue('--n'));
    expect(n).toEqual(['0', '1', '2', '3', '4', '5']);
    expect(Number.parseFloat(line.style.getPropertyValue('--step')) * 5 + 0.7).toBeLessThan(2);
  });

  it('line finish skips an epic done at first read', () => {
    asReduced(false);
    render(EpicLine, { epic: open(102, 'done', 'done') });
    expect(finished(screen.getByTestId('epic-line'))).toBe(0);
  });

  it('line finish does not repeat', async () => {
    asReduced(false);
    const { rerender } = render(EpicLine, { epic: open(103, 'done', 'working') });
    await rerender({ epic: open(103, 'done', 'done') });
    expect(finished(screen.getByTestId('epic-line'))).toBe(6);
    // The next read of the finished epic plays none.
    await rerender({ epic: open(103, 'done', 'done') });
    expect(finished(screen.getByTestId('epic-line'))).toBe(0);
    // Nor does an epic that reopens and finishes again in the same tab.
    await rerender({ epic: open(103, 'done', 'working') });
    await rerender({ epic: open(103, 'done', 'done') });
    expect(finished(screen.getByTestId('epic-line'))).toBe(0);
  });

  it('line finish plays for each project that has an epic of the same number', async () => {
    asReduced(false);
    const a = render(EpicLine, { epic: open(104, 'done', 'working'), project: '/p/a' });
    await a.rerender({ epic: open(104, 'done', 'done'), project: '/p/a' });
    expect(finished(a.container)).toBe(6);
    const b = render(EpicLine, { epic: open(104, 'done', 'working'), project: '/p/b' });
    await b.rerender({ epic: open(104, 'done', 'done'), project: '/p/b' });
    expect(finished(b.container)).toBe(6);
    // Each plays once: the next read of the second plays none.
    await b.rerender({ epic: open(104, 'done', 'done'), project: '/p/b' });
    expect(finished(b.container)).toBe(0);
  });

  it('line finish stops under reduced motion', async () => {
    asReduced(true);
    const { rerender } = render(EpicLine, { epic: open(105, 'done', 'working') });
    await rerender({ epic: open(105, 'done', 'done') });
    expect(finished(screen.getByTestId('epic-line'))).toBe(0);
    // The stylesheet runs the finish only when motion is not reduced.
    const css = readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), 'app.css'), 'utf8');
    const start = css.indexOf('@media (prefers-reduced-motion: no-preference)');
    const end = css.indexOf('@media (prefers-reduced-motion: reduce)');
    expect(css.slice(start, end)).toMatch(/\.square\.finish\s*\{[^}]*animation:\s*square-finish/);
    expect(css.slice(end)).toMatch(/\.square,/);
  });
});
