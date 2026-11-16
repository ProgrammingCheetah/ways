import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/svelte';
import App from './App.svelte';

const machine = {
  instances: 0,
  projects: 1,
  in_use_units: 0,
  budget: { units: null, reserve: 0, sizes: {} },
};

// One item of every kind, as `GET /api/state` lists them.
const items = [
  {
    kind: 'sign',
    project: 'ways',
    title: 'Sign epic #1: The hub page',
    blocks: 6,
    how: 'Read the epic and sign it from a terminal outside any instance.',
    link: 'https://forge.example/zuri/ways/issues/1',
    command: 'cd /home/zuri/ways && zaf epic sign 1',
  },
  {
    kind: 'question',
    project: 'ways',
    title: 'Question #40: Which port?',
    blocks: 3,
    how: 'It proposes: 7777. Rule with the proposal, or with your own answer.',
    link: 'https://forge.example/zuri/ways/issues/40',
    command: "cd /home/zuri/ways && zaf board ruling 40 '7777'",
  },
  {
    kind: 'door',
    project: 'ways',
    title: 'Question #41: Drop the ledger?',
    blocks: 2,
    how: 'A door, a one-way decision: read it twice. It proposes: keep it.',
    link: 'https://forge.example/zuri/ways/issues/41',
    command: "cd /home/zuri/ways && zaf board ruling 41 'keep it'",
  },
  {
    kind: 'unticked',
    project: 'ways',
    title: '2 ready tasks in ways: no scheduler ticks',
    blocks: 2,
    how: 'Start the keeper.',
    link: null,
    command: 'cd /home/zuri/ways && zaf spawn --watch --version <version>',
  },
  {
    kind: 'failed',
    project: 'ways',
    title: 'Question #42 on #9: review got nowhere',
    blocks: 1,
    how: 'It is not started again until this is ruled.',
    link: 'https://forge.example/zuri/ways/issues/42',
    command: "cd /home/zuri/ways && zaf board ruling 42 '<ruling>'",
  },
  {
    kind: 'broken',
    project: 'ways',
    title: 'ways cannot run: FORGEJO_TOKEN is not set',
    blocks: 4,
    how: 'FORGEJO_TOKEN is not set.',
    link: null,
    command: null,
  },
];

function hubAnswers(next) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ machine, next, projects: [] }),
    })),
  );
}

function stubClipboard() {
  const writeText = vi.fn(async () => {});
  vi.stubGlobal('navigator', { clipboard: { writeText } });
  return writeText;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('the Next for you view', () => {
  it('next lists every kind', async () => {
    hubAnswers(items);
    render(App);
    const lines = await screen.findAllByTestId('next-item');
    expect(lines.map((line) => line.dataset.kind)).toEqual([
      'sign',
      'question',
      'door',
      'unticked',
      'failed',
      'broken',
    ]);
    // Each line says what it is and what it blocks.
    expect(lines[0].textContent).toContain('Sign epic #1: The hub page');
    expect(lines[0].textContent).toContain('blocks 6 tasks');
    expect(lines[4].textContent).toContain('blocks 1 task');
    // A question shows its proposed answer; a link opens the thing; a line with neither has neither.
    expect(lines[1].textContent).toContain('It proposes: 7777');
    expect(lines[1].querySelector('a').getAttribute('href')).toBe(
      'https://forge.example/zuri/ways/issues/40',
    );
    expect(lines[3].querySelector('a')).toBeNull();
    expect(lines[3].querySelector('code').textContent).toContain('zaf spawn --watch');
    expect(lines[5].querySelector('code')).toBeNull();
    // It comes before everything else on the page.
    const first = document.querySelector('main').firstElementChild;
    expect(first.querySelector('h2').textContent).toBe('Next for you');
  });

  it('sign line copies command', async () => {
    const writeText = stubClipboard();
    hubAnswers(items);
    render(App);
    const [sign] = await screen.findAllByTestId('next-item');
    expect(sign.querySelector('code').textContent).toBe('cd /home/zuri/ways && zaf epic sign 1');
    expect(sign.querySelector('a').getAttribute('href')).toBe(
      'https://forge.example/zuri/ways/issues/1',
    );
    const button = sign.querySelector('button');
    expect(button.textContent.trim()).toBe('Copy');
    await fireEvent.click(button);
    expect(writeText).toHaveBeenCalledWith('cd /home/zuri/ways && zaf epic sign 1');
    expect((await screen.findByText('Copied')).tagName).toBe('BUTTON');
  });

  it('door marked', async () => {
    hubAnswers(items);
    render(App);
    const lines = await screen.findAllByTestId('next-item');
    const marks = screen.getAllByTestId('door-mark');
    expect(marks).toHaveLength(1);
    expect(lines[2].contains(marks[0])).toBe(true);
    expect(lines[1].querySelector('[data-testid="door-mark"]')).toBeNull();
  });

  it('nothing waiting', async () => {
    hubAnswers([]);
    render(App);
    const line = await screen.findByTestId('nothing-waiting');
    expect(line.textContent).toBe('Nothing is waiting on you.');
    expect(screen.queryAllByTestId('next-item')).toHaveLength(0);
  });
});
