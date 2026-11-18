// Helpers of the epic line, pure so the line and its finish can share them.

const isDone = (task) => task?.state === 'done';

// Compare two reads of an epic's tasks: the numbers of the tasks that were not done and now are, and
// whether every task of the epic just became done (true only on the read where its last task did).
// With no previous read (the page's first) nothing turned done, however much is done.
export function turnedDone(previous, next) {
  if (!Array.isArray(previous) || !Array.isArray(next)) return { numbers: [], epic: false };
  const before = new Map(previous.map((t) => [t.number, t]));
  const numbers = next.filter((t) => isDone(t) && before.has(t.number) && !isDone(before.get(t.number))).map((t) => t.number);
  const wasWhole = previous.length > 0 && previous.every(isDone);
  const epic = numbers.length > 0 && next.length > 0 && next.every(isDone) && !wasWhole;
  return { numbers, epic };
}
