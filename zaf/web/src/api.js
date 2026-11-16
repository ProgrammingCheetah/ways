// The hub's JSON interface: GET /api/state is `{ machine, next, projects }`.

/** Reads the machine state. A refusal is thrown with the hub's own words. */
export async function fetchState() {
  const response = await fetch('/api/state');
  let body = null;
  try {
    body = await response.json();
  } catch {
    // Not JSON: the status below says what went wrong.
  }
  if (!response.ok) {
    throw new Error(body?.refused ?? `the hub answered ${response.status}`);
  }
  return body;
}

/** How long a dropped change stream waits before it is opened again. */
export const RECONNECT_MS = 1000;

/**
 * Holds `GET /api/events` open and calls `onChange` on each `change` event. A dropped stream is
 * closed and opened again after `RECONNECT_MS`. Returns a function that stops watching. Where the
 * browser has no EventSource, nothing is watched.
 */
export function watchChanges(onChange) {
  if (typeof EventSource === 'undefined') return () => {};
  let source = null;
  let timer = null;
  let stopped = false;

  function open() {
    source = new EventSource('/api/events');
    source.addEventListener('change', () => onChange());
    source.onerror = () => {
      source.close();
      if (!stopped && timer === null) {
        timer = setTimeout(() => {
          timer = null;
          open();
        }, RECONNECT_MS);
      }
    };
  }

  open();
  return () => {
    stopped = true;
    clearTimeout(timer);
    source?.close();
  };
}

function count(number, noun) {
  return `${number} ${noun}${number === 1 ? '' : 's'}`;
}

function units(number) {
  return Number.isInteger(number) ? String(number) : number.toFixed(1);
}

/** The machine line as one sentence: what runs on the machine and how much budget it uses. */
export function machineLine(machine) {
  const use =
    machine.budget.units === null
      ? `${units(machine.in_use_units)} units in use, no machine cap`
      : `${units(machine.in_use_units)} of ${machine.budget.units} units in use`;
  return `${count(machine.instances, 'instance')} running · ${count(machine.projects, 'project')} · ${use}`;
}

/** An address a person may open: only http and https, so a board's text never becomes a script. */
export function safeUrl(url) {
  return typeof url === 'string' && /^https?:\/\//i.test(url) ? url : null;
}

/** How long an instance has run, from seconds: `42s`, `12m`, `1h 5m`. Unknown is a dash. */
export function runTime(seconds) {
  if (seconds === null || seconds === undefined) return '–';
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

/** How long ago an RFC 3339 time was, from `now` in ms: `just now`, `5m ago`. The text itself when it is no time. */
export function ago(at, now = Date.now()) {
  const then = Date.parse(at);
  if (Number.isNaN(then)) return String(at);
  const seconds = Math.max(0, Math.round((now - then) / 1000));
  if (seconds < 5) return 'just now';
  return `${runTime(seconds)} ago`;
}

/** An epic's cost, estimated, in dollars. */
export function dollars(number) {
  return `$${(number ?? 0).toFixed(2)}`;
}

/** What a project not read in full opens with: it is missing, or unreachable with its reason. */
export function trouble(project) {
  if (project.missing) return `missing: ${project.reason ?? 'its folder is gone'}`;
  if (project.reason) return `unreachable: ${project.reason}`;
  return null;
}

const KIND_LABELS = {
  sign: 'Sign',
  question: 'Question',
  door: 'Question',
  unticked: 'No scheduler',
  failed: 'Got nowhere',
  broken: 'Cannot run',
};

/** The words for a Next for you item's kind; a kind the page does not know shows as itself. */
export function kindLabel(kind) {
  return KIND_LABELS[kind] ?? kind;
}

/** What an item holds back, in words. */
export function blocksLine(blocks) {
  return blocks === 0 ? 'blocks nothing' : `blocks ${count(blocks, 'task')}`;
}

/** Puts `text` on the clipboard; false when the browser refuses or has no clipboard. */
export async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
