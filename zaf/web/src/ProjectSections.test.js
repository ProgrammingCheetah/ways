import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@testing-library/svelte';
import App from './App.svelte';
import { ago, runTime, safeUrl, trouble } from './api.js';

const machine = {
  instances: 1,
  projects: 3,
  in_use_units: 4,
  budget: { units: 60, reserve: 8, sizes: {} },
};

// A project read in full: the shape `GET /api/state` gives each entry of `projects`.
const full = {
  project: 'ways',
  root: '/home/zuri/code/ai/ways',
  missing: false,
  gathered_at: '2026-11-15T10:00:00Z',
  reason: null,
  epics: [
    {
      number: 1,
      title: 'zaf: the hub becomes a control hub',
      stage: 'challenged',
      url: null,
      done: 21,
      total: 26,
      dollars: 12.5,
      groups: [
        { name: 'api', done: 2, total: 2 },
        { name: 'frontend', done: 1, total: 6 },
      ],
    },
  ],
  queue: [
    {
      number: 22,
      title: 'The page shows one dashboard section per project',
      epic: 1,
      state: 'claimed',
      group: 'frontend',
      instance: 'programmer instance 401 on #1',
      phase: 'build',
      issue_url: 'https://forge.example/zuri/ways/issues/22',
      pull_request: 90,
      pull_request_url: 'https://forge.example/zuri/ways/pulls/90',
    },
    {
      number: 23,
      title: 'The page refreshes itself',
      epic: 1,
      state: 'blocked',
      group: 'frontend',
      instance: null,
      phase: null,
      issue_url: 'javascript:alert(1)',
      pull_request: null,
      pull_request_url: null,
    },
  ],
  questions: [],
  problems: [],
  instances: [
    {
      instance: 'programmer instance 401 on #1',
      role: 'programmer',
      task: 22,
      number: 22,
      title: 'The page shows one dashboard section per project',
      phase: 'build',
      run_seconds: 3725,
      last_tool_call: 'testing',
    },
  ],
  watch: { running: true, pid: 7, since: '2026-11-15T09:00:00Z', last_tick: '2026-11-15T09:59:00Z' },
  last_tick: { at: '2026-11-15T09:59:00Z', started: ['#22'], skipped: ['#23 blocked'] },
};

const gone = {
  ...full,
  project: 'old-project',
  root: '/home/zuri/code/old-project',
  missing: true,
  reason: 'its folder is gone',
  epics: [],
  queue: [],
  instances: [],
  watch: null,
  last_tick: null,
};

const down = {
  ...full,
  project: 'far-project',
  root: '/home/zuri/code/far-project',
  reason: 'the board cannot be reached: connection refused',
  epics: [],
  queue: [],
  instances: [],
  last_tick: null,
};

function hubShows(projects) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ machine, next: [], projects }),
    })),
  );
}

function sectionOf(name) {
  return screen.getAllByTestId('project-section').find((s) => s.getAttribute('aria-label') === name);
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('the project sections', () => {
  it('project sections render', async () => {
    hubShows([full]);
    render(App);
    await screen.findByTestId('project-section');
    const section = sectionOf('ways');
    expect(within(section).queryByTestId('project-trouble')).toBeNull();

    const epics = within(section).getByTestId('epics').textContent;
    expect(epics).toContain('#1');
    expect(epics).toContain('challenged');
    expect(epics).toContain('21 of 26 done');
    expect(epics).toContain('$12.50');
    expect(epics).toContain('api: 2 of 2');
    expect(epics).toContain('frontend: 1 of 6');

    const queue = within(section).getByTestId('queue');
    expect(queue.textContent).toContain('claimed');
    expect(queue.textContent).toContain('epic #1');
    expect(queue.textContent).toContain('build');
    expect(within(queue).getByText('issue').getAttribute('href')).toBe(
      'https://forge.example/zuri/ways/issues/22',
    );
    expect(within(queue).getByText('pull request #90').getAttribute('href')).toBe(
      'https://forge.example/zuri/ways/pulls/90',
    );
    // A link that is no web address is not a link.
    expect(queue.querySelectorAll('a')).toHaveLength(2);

    const instances = within(section).getByTestId('instances').textContent;
    expect(instances).toContain('programmer');
    expect(instances).toContain('#22');
    expect(instances).toContain('build');
    expect(instances).toContain('running 1h 2m');
    expect(instances).toContain('last: testing');

    const ticks = within(section).getByTestId('ticks').textContent;
    expect(ticks).toContain('watch running');
    expect(ticks).toContain('last tick');
    expect(ticks).toContain('started: #22');
    expect(ticks).toContain('skipped: #23 blocked');
  });

  it('missing project shown', async () => {
    hubShows([gone, full]);
    render(App);
    await screen.findAllByTestId('project-section');
    const section = sectionOf('old-project');
    expect(within(section).getByTestId('project-trouble').textContent.trim()).toBe(
      'missing: its folder is gone',
    );
    expect(within(section).queryByTestId('epics')).toBeNull();
    // The other project still shows.
    expect(within(sectionOf('ways')).getByTestId('epics')).toBeTruthy();
  });

  it('unreachable reason shown', async () => {
    hubShows([down, full]);
    render(App);
    await screen.findAllByTestId('project-section');
    const section = sectionOf('far-project');
    expect(within(section).getByTestId('project-trouble').textContent.trim()).toBe(
      'unreachable: the board cannot be reached: connection refused',
    );
    expect(within(sectionOf('ways')).getByTestId('queue')).toBeTruthy();
  });

  it('shows no section for no project', async () => {
    hubShows([]);
    render(App);
    await screen.findByTestId('machine-line');
    expect(screen.queryAllByTestId('project-section')).toHaveLength(0);
  });
});

describe('the section helpers', () => {
  it('words a run time', () => {
    expect(runTime(42)).toBe('42s');
    expect(runTime(720)).toBe('12m');
    expect(runTime(3725)).toBe('1h 2m');
    expect(runTime(null)).toBe('–');
  });

  it('words how long ago', () => {
    const now = Date.parse('2026-11-15T10:00:00Z');
    expect(ago('2026-11-15T09:55:00Z', now)).toBe('5m ago');
    expect(ago('2026-11-15T09:59:59Z', now)).toBe('just now');
    expect(ago('not a time', now)).toBe('not a time');
  });

  it('opens only web addresses', () => {
    expect(safeUrl('https://a.example/x')).toBe('https://a.example/x');
    expect(safeUrl('javascript:alert(1)')).toBeNull();
    expect(safeUrl(null)).toBeNull();
  });

  it('tells missing from unreachable', () => {
    expect(trouble({ missing: false, reason: null })).toBeNull();
    expect(trouble({ missing: true, reason: 'its manifest is gone' })).toBe('missing: its manifest is gone');
    expect(trouble({ missing: false, reason: 'down' })).toBe('unreachable: down');
  });
});
