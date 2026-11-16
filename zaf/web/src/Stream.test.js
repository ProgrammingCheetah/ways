import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import App from './App.svelte';
import { RECONNECT_MS, watchChanges } from './api.js';

// A stand-in for the browser's EventSource: tests fire its events by hand.
class FakeSource {
  static all = [];
  constructor(url) {
    this.url = url;
    this.closed = false;
    this.listeners = {};
    this.onerror = null;
    FakeSource.all.push(this);
  }
  addEventListener(type, handler) {
    this.listeners[type] = handler;
  }
  close() {
    this.closed = true;
  }
  emit(type) {
    this.listeners[type]?.({ type });
  }
}

function stateWith(instances) {
  return {
    machine: { instances, projects: 1, in_use_units: 0, budget: { units: null } },
    next: [],
    projects: [],
  };
}

beforeEach(() => {
  FakeSource.all = [];
  vi.stubGlobal('EventSource', FakeSource);
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('the change stream', () => {
  it('change event redraws', async () => {
    const answers = [stateWith(1), stateWith(2)];
    const fetchMock = vi.fn(async () => ({ ok: true, status: 200, json: async () => answers.shift() }));
    vi.stubGlobal('fetch', fetchMock);

    render(App);
    const line = await screen.findByTestId('machine-line');
    expect(line.textContent).toContain('1 instance running');
    expect(FakeSource.all).toHaveLength(1);
    expect(FakeSource.all[0].url).toBe('/api/events');
    expect(fetchMock).toHaveBeenCalledTimes(1);

    FakeSource.all[0].emit('change');
    await waitFor(() =>
      expect(screen.getByTestId('machine-line').textContent).toContain('2 instances running'),
    );
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(fetchMock).toHaveBeenLastCalledWith('/api/state');
  });

  it('stream reconnects', () => {
    vi.useFakeTimers();
    const onChange = vi.fn();
    const stop = watchChanges(onChange);
    expect(FakeSource.all).toHaveLength(1);

    FakeSource.all[0].onerror();
    expect(FakeSource.all[0].closed).toBe(true);
    expect(FakeSource.all).toHaveLength(1);

    vi.advanceTimersByTime(RECONNECT_MS);
    expect(FakeSource.all).toHaveLength(2);
    FakeSource.all[1].emit('change');
    expect(onChange).toHaveBeenCalledTimes(1);

    stop();
    FakeSource.all[1].onerror();
    vi.advanceTimersByTime(RECONNECT_MS * 2);
    expect(FakeSource.all).toHaveLength(2);
  });
});
