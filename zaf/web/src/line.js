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

// The squares each size stands for, as the line counts them; a task with no size, or one this page does
// not know, takes UNKNOWN_SIZE.
const SIZES = { XS: 1, S: 2, M: 3, L: 4, XL: 5, E: 6 };
const UNKNOWN_SIZE = 2;
const MAX_ROWS = 7;

// The rows of the grid for a number of squares: the most rows n, 1 to 7, that still leave fewer rows
// than columns (n < ceil(squares / n)); 1 for one square or none, and when no n qualifies.
export function gridRows(squares) {
  const total = Number.isFinite(squares) ? Math.max(0, Math.floor(squares)) : 0;
  for (let n = MAX_ROWS; n > 1; n--) {
    if (n < Math.ceil(total / n)) return n;
  }
  return 1;
}

// One entry for each square of the tasks, in task order, a task's squares one after the other:
// [{ task, index, column, row }], `index` being the square's place within its task. The squares run
// down a column and on to the next, and a task starts right after the previous task's last square.
export function gridCells(tasks) {
  const list = Array.isArray(tasks) ? tasks : [];
  const counts = list.map((task) => SIZES[task?.size] ?? UNKNOWN_SIZE);
  const rows = gridRows(counts.reduce((sum, n) => sum + n, 0));
  const cells = [];
  list.forEach((task, t) => {
    for (let index = 0; index < counts[t]; index++) {
      const place = cells.length;
      cells.push({ task, index, column: Math.floor(place / rows), row: place % rows });
    }
  });
  return cells;
}
