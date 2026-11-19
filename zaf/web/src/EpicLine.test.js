import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import { forgetKey, takeKey } from './controls.js';
import EpicLine from './EpicLine.svelte';
import { gridCells, gridRows, turnedDone } from './line.js';

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
    const blocks = screen.getAllByTestId('task-block');
    expect(blocks.map((b) => b.dataset.task)).toEqual(['1', '2', '3']);
    expect(blocks.map((b) => squares(b).length)).toEqual([1, 3, 2]);
  });

  it('epic line draws the epic\'s square beside the title', () => {
    render(EpicLine, { epic: epic([task(1, 'done', 'XS'), task(2, 'working', 'M')]) });
    const own = screen.getByTestId('epic-block');
    // Exactly one square, in the title row and not in the grid.
    expect(squares(own)).toHaveLength(1);
    const title = document.querySelector('.line-title');
    expect(title.contains(own)).toBe(true);
    expect(title.querySelector('.title').textContent).toBe('an epic');
    expect(screen.getByTestId('epic-grid').contains(own)).toBe(false);
    expect(squares(screen.getByTestId('epic-grid'))).toHaveLength(4);
  });

  it('epic line places each square at its grid row and column', () => {
    render(EpicLine, { epic: epic([task(1, 'done', 'XS'), task(2, 'working', 'M'), task(3, 'not_started', null)]) });
    const grid = screen.getByTestId('epic-grid');
    const rows = gridRows(6);
    expect(grid.style.getPropertyValue('--rows')).toBe(String(rows));
    const placed = [...squares(grid)].map((s) => [s.style.gridRow, s.style.gridColumn]);
    const cells = gridCells([task(1, 'done', 'XS'), task(2, 'working', 'M'), task(3, 'not_started', null)]);
    expect(placed).toEqual(cells.map((c) => [String(c.row + 1), String(c.column + 1)]));
  });

  it('epic line draws 50 squares as 7 rows by 8 columns', () => {
    // Twenty-five S tasks are 50 squares.
    const tasks = Array.from({ length: 25 }, (_, i) => task(i + 1, 'not_started', 'S'));
    render(EpicLine, { epic: epic(tasks) });
    const grid = screen.getByTestId('epic-grid');
    expect(grid.dataset.rows).toBe('7');
    const all = [...squares(grid)];
    expect(all).toHaveLength(50);
    expect(Math.max(...all.map((s) => Number(s.dataset.row)))).toBe(7);
    expect(Math.max(...all.map((s) => Number(s.dataset.column)))).toBe(8);
  });

  it('epic line uses one gap for every square', () => {
    const css = readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), 'app.css'), 'utf8');
    render(EpicLine, { epic: epic([task(1, 'done', 'S'), task(2, 'done', 'M')]) });
    // No task-group wrapper class and no per-task spacing in the page or the stylesheet.
    expect(document.querySelector('.task-group, .group, .run')).toBeNull();
    expect(css).not.toMatch(/gap:\s*1px/);
    expect(css).not.toMatch(/gap:\s*0\.35rem 0\.25rem/);
    // The grid's one gap is the shared variable; the epic's one square sits outside the grid.
    const gaps = [...css.matchAll(/^\s*gap:\s*var\(--sq-gap\);/gm)];
    expect(gaps).toHaveLength(1);
    expect(css).toMatch(/\.grid\s*\{[^}]*gap:\s*var\(--sq-gap\)/);
    expect(css.match(/--sq-gap:\s*[^;]+;/g)).toHaveLength(1);
    for (const s of squares(screen.getByTestId('epic-grid'))) expect(s.style.margin).toBe('');
  });

  it('epic line scrolls its grid sideways in its own box', () => {
    const css = readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), 'app.css'), 'utf8');
    render(EpicLine, { epic: epic([task(1, 'done', 'S')]) });
    const box = screen.getByTestId('epic-grid-box');
    expect(box.contains(screen.getByTestId('epic-grid'))).toBe(true);
    expect(css).toMatch(/\.grid-box\s*\{[^}]*overflow-x:\s*auto/);
  });

  it('the_grid_box_leaves_room_for_outlines_and_motion', () => {
    const css = readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), 'app.css'), 'utf8');
    const rule = css.match(/\.grid-box\s*\{([^}]*)\}/)[1];
    expect(rule).toMatch(/overflow-x:\s*auto/);
    expect(rule).toMatch(/padding:\s*6px\s*;/);
    expect(rule).toMatch(/margin:\s*-6px\s*;/);
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
    const own = screen.getByTestId('task-block');
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
    const [first, , last] = squares(block);
    // Any square of the task shows its card: the first, or a later one.
    await fireEvent.mouseEnter(last);
    const card = screen.getByTestId('line-card');
    expect(card.textContent).toContain('#5');
    expect(card.textContent).toContain('build it');
    expect(card.textContent).toContain('size M');
    expect(card.textContent).toContain('working');
    expect(card.textContent).toContain('phase implement');
    // The card is tied to the task's one stop, and closes when the mouse leaves.
    expect(within(block).getByRole('link').getAttribute('aria-describedby')).toBe(card.id);
    await fireEvent.mouseLeave(last);
    expect(screen.queryByTestId('line-card')).toBeNull();
    await fireEvent.mouseEnter(first);
    expect(screen.getByTestId('line-card')).toBeTruthy();
  });

  it('line card hangs from the square under the pointer', async () => {
    const rect = (left) => ({ left, top: 10, right: left + 14, bottom: 24, width: 14, height: 14 });
    const proto = Element.prototype.getBoundingClientRect;
    Element.prototype.getBoundingClientRect = function () {
      return this.classList?.contains('square') ? rect(20 * Number(this.dataset.column)) : proto.call(this);
    };
    try {
      render(EpicLine, { epic: epic([task(5, 'working', 'M', { issue_url: url(5) })]) });
      const all = squares(screen.getByTestId('task-block'));
      const place = () => Number.parseFloat(screen.getByTestId('line-card').style.left);
      await fireEvent.mouseEnter(all[0]);
      const first = place();
      await fireEvent.mouseLeave(all[0]);
      await fireEvent.mouseEnter(all[2]);
      expect(place()).toBe(first + 20 * (Number(all[2].dataset.column) - Number(all[0].dataset.column)));
    } finally {
      Element.prototype.getBoundingClientRect = proto;
    }
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

  it('scrolling_the_grid_box_hides_the_card', async () => {
    render(EpicLine, { epic: epic([task(5, 'working', 'M', { issue_url: url(5) })]) });
    const [first] = squares(screen.getByTestId('task-block'));
    await fireEvent.mouseEnter(first);
    expect(screen.getByTestId('line-card')).toBeTruthy();
    await fireEvent.scroll(screen.getByTestId('epic-grid-box'));
    expect(screen.queryByTestId('line-card')).toBeNull();
  });

  it('the window scroll and resize still hide the card', async () => {
    render(EpicLine, { epic: epic([task(5, 'working', 'M', { issue_url: url(5) })]) });
    const [first] = squares(screen.getByTestId('task-block'));
    for (const event of ['scroll', 'resize']) {
      await fireEvent.mouseEnter(first);
      expect(screen.getByTestId('line-card')).toBeTruthy();
      await fireEvent(window, new Event(event));
      expect(screen.queryByTestId('line-card')).toBeNull();
    }
  });

  it('line card lists a blocked task\'s blockers', async () => {
    const blocked_by = [
      { number: 3, title: 'first part', state: 'working' },
      { number: 4, title: null, state: 'not_started' },
    ];
    render(EpicLine, { epic: epic([task(9, 'blocked', 'S', { blocked_by })]) });
    await fireEvent.mouseEnter(squares(screen.getByTestId('task-block'))[0]);
    const items = within(screen.getByTestId('card-blockers')).getAllByRole('listitem');
    expect(items.map((li) => li.textContent.trim())).toEqual(['#3 first part: working', '#4: not started']);
  });

  it('line card shows a purple task\'s question, as text', async () => {
    const question = { number: 20, text: 'Which <b>door</b>?' };
    render(EpicLine, { epic: epic([task(8, 'question', 'S', { question })]) });
    await fireEvent.mouseEnter(squares(screen.getByTestId('task-block'))[0]);
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

describe('line focus', () => {
  const url = (n) => `https://example.test/issues/${n}`;
  const stops = (root) =>
    [...root.querySelectorAll('a[href], button, [tabindex]')].filter((el) => el.getAttribute('tabindex') !== '-1');
  const mixed = () =>
    epic([
      task(1, 'done', 'M', { issue_url: url(1) }),
      task(2, 'blocked', 'S'),
      task(3, 'question', 'L', { issue_url: url(3), question: { number: 20, text: 'Which?' } }),
    ]);

  it('line focus has one stop per task', () => {
    render(EpicLine, { epic: mixed(), controls: true, project: '/p' });
    for (const block of screen.getAllByTestId('task-block')) {
      const found = stops(block);
      expect(found).toHaveLength(1);
      // Its first square, carrying the task's name.
      expect(found[0]).toBe(squares(block)[0]);
      expect(within(found[0]).getByRole('img').getAttribute('aria-label')).toMatch(/^Task #/);
    }
    expect(screen.getByRole('link', { name: 'Task #1 task 1: done' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Task #3 task 3: has a question open' })).toBeTruthy();
    expect(screen.getAllByRole('link')).toHaveLength(1);
    // A task with no address is still its one stop, opening nothing.
    expect(squares(screen.getAllByTestId('task-block')[1])[0].getAttribute('tabindex')).toBe('0');
  });

  it('line focus hides a task\'s other squares', () => {
    render(EpicLine, { epic: mixed() });
    for (const block of screen.getAllByTestId('task-block')) {
      const [, ...rest] = squares(block);
      expect(rest.length).toBeGreaterThan(0);
      for (const s of rest) {
        expect(s.getAttribute('aria-hidden')).toBe('true');
        expect(s.hasAttribute('tabindex')).toBe(false);
        expect(s.matches('a, button, [href]')).toBe(false);
        expect(s.querySelector('a, button, [tabindex]')).toBeNull();
      }
    }
    expect(screen.getAllByTestId('task-block').every((b) => b.tagName === 'LI')).toBe(true);
  });

  it('line focus keeps the ordered list and no display contents', () => {
    const css = readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), 'app.css'), 'utf8');
    expect(css).not.toMatch(/display:\s*contents/);
    render(EpicLine, { epic: mixed() });
    expect(screen.getByTestId('epic-grid').tagName).toBe('UL');
  });

  it('line hover outlines a task\'s squares', async () => {
    render(EpicLine, { epic: mixed() });
    const [one, two] = screen.getAllByTestId('task-block');
    const outlined = () => [...document.querySelectorAll('.square.outlined')];
    expect(outlined()).toHaveLength(0);
    const [, second] = squares(one);
    await fireEvent.mouseEnter(second);
    // Every square of that task, the one hovered included, and none of another task's.
    expect(outlined()).toEqual([...squares(one)]);
    await fireEvent.mouseLeave(second);
    expect(outlined()).toHaveLength(0);
    await fireEvent.mouseEnter(squares(two)[0]);
    expect(outlined()).toEqual([...squares(two)]);
    // Neither the epic's square nor the legend's swatches are outlined.
    expect(squares(screen.getByTestId('epic-block'))[0].classList.contains('outlined')).toBe(false);
  });

  it('line click on a later square goes where the first goes', async () => {
    render(EpicLine, { epic: mixed() });
    const [one] = screen.getAllByTestId('task-block');
    const first = squares(one)[0];
    const clicks = [];
    first.addEventListener('click', (e) => {
      clicks.push(e);
      e.preventDefault();
    });
    const later = squares(one)[2];
    await fireEvent.click(later, { ctrlKey: true });
    expect(clicks).toHaveLength(1);
    expect(clicks[0].target).toBe(first);
    expect(clicks[0].ctrlKey).toBe(true);
    expect(first.getAttribute('href')).toBe(url(1));
  });

  it('line click on a later square opens the same prompt', async () => {
    window.history.replaceState(null, '', '/#key=k3y-0123456789');
    forgetKey();
    takeKey();
    try {
      render(EpicLine, { epic: mixed(), controls: true, project: '/p' });
      const block = screen.getAllByTestId('task-block')[2];
      await fireEvent.click(squares(block)[3]);
      expect(screen.getByTestId('prompt-question').textContent).toBe('Which?');
      await fireEvent.click(screen.getByTestId('prompt-close'));
      expect(document.activeElement).toBe(squares(block)[0]);
    } finally {
      forgetKey();
      window.history.replaceState(null, '', '/');
    }
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

  it('line prompt opens on a purple task with the key', async () => {
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

  it('line prompt opens on the epic when it is purple', async () => {
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

  it('line purple task opens its issue without the key', async () => {
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
    // Each task's two squares and the epic's one.
    expect(finished(line)).toBe(5);
    expect(Number.parseFloat(line.style.getPropertyValue('--step')) * 4 + 0.7).toBeLessThan(2);
  });

  it('line finish shines the epic\'s square last', async () => {
    asReduced(false);
    const { rerender } = render(EpicLine, { epic: open(106, 'done', 'working') });
    await rerender({ epic: open(106, 'done', 'done') });
    // The grid's squares are reached in grid order: down a column, then the next.
    const grid = [...squares(screen.getByTestId('epic-grid'))].sort(
      (a, b) => a.dataset.column - b.dataset.column || a.dataset.row - b.dataset.row,
    );
    expect(grid.map((s) => s.style.getPropertyValue('--n'))).toEqual(['0', '1', '2', '3']);
    // The epic's square comes after the grid's last.
    const own = squares(screen.getByTestId('epic-block'));
    expect(own).toHaveLength(1);
    expect(own[0].classList.contains('finish')).toBe(true);
    expect(own[0].style.getPropertyValue('--n')).toBe('4');
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
    expect(finished(screen.getByTestId('epic-line'))).toBe(5);
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
    expect(finished(a.container)).toBe(5);
    const b = render(EpicLine, { epic: open(104, 'done', 'working'), project: '/p/b' });
    await b.rerender({ epic: open(104, 'done', 'done'), project: '/p/b' });
    expect(finished(b.container)).toBe(5);
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
