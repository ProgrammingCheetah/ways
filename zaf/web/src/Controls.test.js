import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import App from './App.svelte';
import { forgetKey, proposalOf } from './controls.js';

const KEY = 'k3y-0123456789';
const ROOT = '/home/zuri/code/ai/ways';

const machine = {
  instances: 1,
  projects: 2,
  in_use_units: 4,
  budget: { units: 60, reserve: 8, sizes: {} },
};

const project = {
  project: 'ways',
  root: ROOT,
  missing: false,
  gathered_at: '2026-11-15T10:00:00Z',
  reason: null,
  epics: [],
  queue: [],
  questions: [
    {
      number: 31,
      raised_on: 5,
      door: false,
      url: null,
      text: 'Which way?\nClaude\'s answer (2026-11-15): Take the left road.',
    },
  ],
  problems: [],
  instances: [
    {
      instance: 'programmer instance 401 on #1',
      role: 'programmer',
      task: 22,
      number: 22,
      title: 'A task',
      phase: 'build',
      run_seconds: 30,
      last_tool_call: 'testing',
    },
  ],
  watch: { running: false, pid: null, since: null, version: null, last_tick: null },
  last_tick: null,
};

const gone = {
  ...project,
  project: 'old',
  root: '/home/zuri/code/old',
  missing: true,
  reason: 'its folder is gone',
  questions: [],
  instances: [],
  watch: null,
};

// The hub: GET /api/state answers the fixture; a POST answers what `reply` says for its route.
function hub({ controls = true, projects = [project], reply = {} } = {}) {
  const posts = [];
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url, init) => {
      if (url === '/api/state') {
        return { ok: true, status: 200, json: async () => ({ machine, next: [], projects, controls }) };
      }
      posts.push({ url, init, body: JSON.parse(init.body) });
      const made = reply[url] ?? { status: 200, body: {} };
      return { ok: made.status < 400, status: made.status, json: async () => made.body };
    }),
  );
  return posts;
}

function open(fragment = `#key=${KEY}`) {
  window.history.replaceState(null, '', `/${fragment}`);
}

async function section(name = 'ways') {
  await screen.findAllByTestId('project-section');
  return screen.getAllByTestId('project-section').find((s) => s.getAttribute('aria-label') === name);
}

beforeEach(() => {
  forgetKey();
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  window.history.replaceState(null, '', '/');
});

describe('the control buttons', () => {
  it('answer button', async () => {
    const posts = hub({
      reply: { '/api/answer': { status: 200, body: { answer: '#31 ruled; #5 ready', question: 31, ready: 5 } } },
    });
    open();
    render(App);
    const card = await section();
    await fireEvent.click(within(card).getByTestId('answer-accept'));
    expect(await within(card).findByText('#31 ruled; #5 ready')).toBeTruthy();
    expect(posts).toHaveLength(1);
    expect(posts[0].url).toBe('/api/answer');
    expect(posts[0].init.method).toBe('POST');
    expect(posts[0].init.headers['X-Zaf-Key']).toBe(KEY);
    expect(posts[0].body).toEqual({ project: ROOT, issue: 31, ruling: 'Take the left road.' });

    // Edit: the answer is the Engineer's own.
    await fireEvent.click(within(card).getByTestId('answer-edit'));
    const text = within(card).getByTestId('answer-text');
    expect(text.value).toBe('Take the left road.');
    await fireEvent.input(text, { target: { value: 'Take the right road.' } });
    await fireEvent.click(within(card).getByTestId('answer-send'));
    await waitFor(() => expect(posts).toHaveLength(2));
    expect(posts[1].body).toEqual({ project: ROOT, issue: 31, ruling: 'Take the right road.' });
    expect(posts[1].init.headers['X-Zaf-Key']).toBe(KEY);
  });

  it('tick asks version once', async () => {
    const posts = hub({
      reply: {
        '/api/tick': { status: 200, body: { version: '0.4.0', running: false, tick: ['started #22'] } },
      },
    });
    const prompt = vi.spyOn(window, 'prompt').mockReturnValue(' 0.4.0 ');
    open();
    render(App);
    const card = await section();
    await fireEvent.click(within(card).getByTestId('tick'));
    expect(await within(card).findByText('ticked for 0.4.0: started #22')).toBeTruthy();
    await fireEvent.click(within(card).getByTestId('tick'));
    await waitFor(() => expect(posts).toHaveLength(2));
    expect(prompt).toHaveBeenCalledTimes(1);
    expect(posts[0].url).toBe('/api/tick');
    expect(posts[0].init.headers['X-Zaf-Key']).toBe(KEY);
    expect(posts[0].body).toEqual({ project: ROOT, version: '0.4.0' });
    expect(posts[1].body).toEqual({ project: ROOT });
  });

  it('tick runs nothing without a version', async () => {
    const posts = hub();
    vi.spyOn(window, 'prompt').mockReturnValue(null);
    open();
    render(App);
    const card = await section();
    await fireEvent.click(within(card).getByTestId('tick'));
    expect((await within(card).findByRole('alert')).textContent).toContain('no version given');
    expect(posts).toHaveLength(0);
  });

  it('watch toggle', async () => {
    const posts = hub({
      reply: {
        '/api/watch': { status: 200, body: { project: ROOT, action: 'start', watching: true, version: '0.4.0' } },
      },
    });
    vi.spyOn(window, 'prompt').mockReturnValue('0.4.0');
    open();
    const first = render(App);
    const card = await section();
    await fireEvent.click(within(card).getByText('Start the watch'));
    await waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0].url).toBe('/api/watch');
    expect(posts[0].init.headers['X-Zaf-Key']).toBe(KEY);
    expect(posts[0].body).toEqual({ project: ROOT, action: 'start', version: '0.4.0' });
    first.unmount();

    // A running watch offers its stop.
    hub({
      projects: [{ ...project, watch: { ...project.watch, running: true, version: '0.4.0' } }],
      reply: { '/api/watch': { status: 202, body: { watching: true, stopping: 'it ends once its current tick has' } } },
    });
    open();
    render(App);
    const running = await section();
    expect(within(running).queryByText('Start the watch')).toBeNull();
    await fireEvent.click(within(running).getByText('Stop the watch'));
    expect(await within(running).findByText(/it ends once its current tick has/)).toBeTruthy();
    const stop = fetch.mock.calls.find(([url]) => url === '/api/watch');
    expect(JSON.parse(stop[1].body)).toEqual({ project: ROOT, action: 'stop' });
    expect(stop[1].headers['X-Zaf-Key']).toBe(KEY);
  });

  it('stop button', async () => {
    const posts = hub({
      reply: { '/api/stop': { status: 200, body: { task: 22, stopped: [{ instance: 'x' }], released: ['x'] } } },
    });
    open();
    render(App);
    const card = await section();
    await fireEvent.click(within(card).getByTestId('stop'));
    expect(await within(card).findByText('#22 stopped: 1 instance ended, 1 claim released')).toBeTruthy();
    expect(posts[0].url).toBe('/api/stop');
    expect(posts[0].init.headers['X-Zaf-Key']).toBe(KEY);
    expect(posts[0].body).toEqual({ project: ROOT, task: 22 });
  });

  it('drop button', async () => {
    const posts = hub({
      projects: [gone, project],
      reply: { '/api/drop': { status: 200, body: { dropped: '/home/zuri/code/old' } } },
    });
    open();
    render(App);
    const card = await section('old');
    // Only a missing project can be dropped.
    expect(within(await section('ways')).queryByTestId('drop')).toBeNull();
    await fireEvent.click(within(card).getByTestId('drop'));
    expect(await within(card).findByText('dropped /home/zuri/code/old from the list')).toBeTruthy();
    expect(posts[0].url).toBe('/api/drop');
    expect(posts[0].init.headers['X-Zaf-Key']).toBe(KEY);
    expect(posts[0].body).toEqual({ project: '/home/zuri/code/old' });
  });

  it('refusal shown', async () => {
    hub({
      reply: {
        '/api/stop': { status: 409, body: { refused: '#22 has no live instance; nothing was changed' } },
      },
    });
    open();
    render(App);
    const card = await section();
    await fireEvent.click(within(card).getByTestId('stop'));
    const alert = await within(card).findByRole('alert');
    expect(alert.textContent).toBe('#22 has no live instance; nothing was changed');
    expect(card.textContent).not.toContain(KEY);
  });

  it('read-only hides controls', async () => {
    hub({ controls: false });
    open();
    render(App);
    const card = await section();
    expect(within(card).queryAllByRole('button')).toHaveLength(0);
    expect(within(card).getByTestId('questions')).toBeTruthy();
  });

  it('no key hides controls', async () => {
    hub();
    open('');
    render(App);
    const card = await section();
    expect(within(card).queryAllByRole('button')).toHaveLength(0);
  });

  it('key stripped from address', async () => {
    hub();
    open(`?a=1#key=${KEY}&view=wide`);
    render(App);
    await section();
    expect(window.location.href).not.toContain(KEY);
    expect(window.location.hash).toBe('#view=wide');
    expect(window.location.search).toBe('?a=1');
    expect(JSON.stringify(localStorage)).not.toContain(KEY);
    expect(JSON.stringify(sessionStorage)).not.toContain(KEY);
    // The key still works after the address lost it.
    expect(screen.getAllByRole('button').length).toBeGreaterThan(0);
  });
});

describe('proposalOf', () => {
  it('reads the proposed answer as the hub does', () => {
    expect(proposalOf("Why?\n**Claude's answer (2026-11-13; the Engineer adjusts):** Yes.")).toBe('Yes.');
    expect(proposalOf('- Proposal: do it')).toBe('do it');
    expect(proposalOf('Proposed answer:')).toBeNull();
    expect(proposalOf('no proposal here')).toBeNull();
  });
});
