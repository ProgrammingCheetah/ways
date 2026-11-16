import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import App from './App.svelte';
import { machineLine } from './api.js';

// What a fixture hub answers to GET /api/state: the machine line's fields, no projects yet.
const fixture = {
  machine: {
    instances: 2,
    projects: 3,
    in_use_units: 34.5,
    budget: { units: 60, reserve: 8, sizes: {} },
  },
  next: [],
  projects: [],
};

function hubAnswers(status, body) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => ({ ok: status < 400, status, json: async () => body })),
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('the hub page', () => {
  it('machine line renders', async () => {
    hubAnswers(200, fixture);
    render(App);
    const line = await screen.findByTestId('machine-line');
    expect(line.textContent).toBe('2 instances running · 3 projects · 34.5 of 60 units in use');
    expect(fetch).toHaveBeenCalledWith('/api/state');
  });

  it('says why the state is refused', async () => {
    hubAnswers(503, { refused: 'no zaf home' });
    render(App);
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toBe('no zaf home');
  });
});

describe('machineLine', () => {
  it('names a machine with no cap', () => {
    const machine = { ...fixture.machine, instances: 1, projects: 1, budget: { units: null } };
    expect(machineLine(machine)).toBe('1 instance running · 1 project · 34.5 units in use, no machine cap');
  });
});
