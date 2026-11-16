// The hub's controls: `POST /api/<control>` with the key the page was opened with.
//
// The key arrives in the link's fragment (`/#key=<key>`), which a browser never sends. The page
// reads it once, keeps it here in memory for the tab (never in local storage, a cookie or the
// DOM) and strips it from the address bar. It travels only in the `X-Zaf-Key` request header.

const KEY_HEADER = 'X-Zaf-Key';
const LINK_PARAMETER = 'key';

let key = null;
// The projects this tab gave a version for: the hub remembers it, so it is asked only once.
const versioned = new Set();

/** Reads the key from the address's fragment, keeps it in memory and strips it from the address. */
export function takeKey() {
  const params = new URLSearchParams(window.location.hash.replace(/^#/, ''));
  if (params.has(LINK_PARAMETER)) {
    const found = params.get(LINK_PARAMETER);
    if (found) key = found;
    params.delete(LINK_PARAMETER);
    const rest = params.toString();
    const address = window.location.pathname + window.location.search + (rest ? `#${rest}` : '');
    window.history.replaceState(window.history.state, '', address);
  }
  return key !== null;
}

/** Whether this tab holds a key. */
export function hasKey() {
  return key !== null;
}

/** Forgets the key and the versions given: a fresh tab. */
export function forgetKey() {
  key = null;
  versioned.clear();
}

/**
 * Sends one control with the key. The answer is the hub's JSON; a refusal or a failure is thrown
 * with the hub's own reason, and never with the key.
 */
export async function control(route, body) {
  if (key === null) {
    throw new Error('this page has no key: open the link that `zaf hub link` prints');
  }
  let response;
  try {
    response = await fetch(`/api/${route}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', [KEY_HEADER]: key },
      body: JSON.stringify(body),
    });
  } catch {
    throw new Error('the hub did not answer');
  }
  let reply = null;
  try {
    reply = await response.json();
  } catch {
    // Not JSON: a refusal's status says what went wrong, and a success is not taken as one.
  }
  if (!response.ok) {
    throw new Error(reply?.refused ?? `the hub answered ${response.status}`);
  }
  if (reply === null || typeof reply !== 'object') {
    throw new Error(`the hub answered ${response.status} with no JSON: the control may not have run`);
  }
  return reply;
}

/**
 * The version to send: asked for the first time this tab runs a tick or a watch for the project,
 * and none after, since the hub remembers the one given. A prompt left empty or cancelled runs
 * nothing.
 */
function versionToSend(project, name, suggestion) {
  if (versioned.has(project)) return undefined;
  const given = window.prompt(
    `Which version does ${name} build toward? The hub remembers it. For example 0.4.0`,
    suggestion ?? '',
  );
  const version = typeof given === 'string' ? given.trim() : '';
  if (version === '') throw new Error('no version given: nothing was run');
  return version;
}

/** Records the answer to a question: the proposed answer as it stands, or the Engineer's own. */
export async function answer(project, issue, ruling) {
  const reply = await control('answer', { project, issue, ruling });
  return reply.answer ?? `#${issue} ruled`;
}

/** Runs one tick of a project, asking for its version the first time. */
export async function tick(project, name) {
  const version = versionToSend(project, name);
  const reply = await control('tick', version === undefined ? { project } : { project, version });
  versioned.add(project);
  const said = Array.isArray(reply.tick) ? reply.tick.join('; ') : 'the tick ran';
  return `ticked for ${reply.version}: ${said}`;
}

/** Starts the project's watch, asking for its version the first time. */
export async function startWatch(project, name, suggestion) {
  const version = versionToSend(project, name, suggestion);
  const body = { project, action: 'start' };
  if (version !== undefined) body.version = version;
  const reply = await control('watch', body);
  versioned.add(project);
  return `watching for ${reply.version}; ${reply.tick ?? 'the first tick runs at once'}`;
}

/** Stops the project's watch; it ends after its current tick. */
export async function stopWatch(project) {
  const reply = await control('watch', { project, action: 'stop' });
  return reply.stopping ? `stopping: ${reply.stopping}` : 'the watch stopped';
}

/** Stops the instance running a task and releases its claim. */
export async function stopTask(project, task) {
  const reply = await control('stop', { project, task });
  const stopped = Array.isArray(reply.stopped) ? reply.stopped.length : 0;
  const released = Array.isArray(reply.released) ? reply.released.length : 0;
  return `#${task} stopped: ${stopped} instance${stopped === 1 ? '' : 's'} ended, ${released} claim${released === 1 ? '' : 's'} released`;
}

/** Takes a missing project off the hub's list; its folder is never touched. */
export async function dropProject(project) {
  const reply = await control('drop', { project });
  return `dropped ${reply.dropped ?? project} from the list`;
}

/**
 * The answer a question proposes: what follows a line that opens with `Claude's answer`,
 * `Proposed answer` or `Proposal`, as the hub's Next for you list reads it. None when no such
 * line holds an answer.
 */
export function proposalOf(text) {
  const openings = ["claude's answer", 'proposed answer', 'proposal'];
  for (const raw of String(text).split('\n')) {
    const line = raw.trim().replace(/^[*\- ]+/, '');
    const lower = line.toLowerCase();
    const opening = openings.find((o) => lower.startsWith(o));
    if (!opening) continue;
    let rest = line.slice(opening.length);
    if (rest.trimStart().startsWith('(')) {
      const close = rest.indexOf(')');
      if (close < 0) continue;
      rest = rest.slice(close + 1);
    }
    rest = rest.trimStart();
    if (!rest.startsWith(':')) continue;
    const found = rest.slice(1).replace(/^[* ]+/, '').trim();
    if (found) return found;
  }
  return null;
}
