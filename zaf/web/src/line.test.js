import { describe, expect, it } from 'vitest';
import { gridCells, gridRows } from './line.js';

const columnsOf = (squares) => Math.ceil(squares / gridRows(squares));
const task = (number, size) => ({ number, size });

describe('the grid of an epic', () => {
  it('grid rows for 1, 12, 50 and 120 squares', () => {
    expect([gridRows(1), columnsOf(1)]).toEqual([1, 1]);
    expect([gridRows(12), columnsOf(12)]).toEqual([3, 4]);
    expect([gridRows(50), columnsOf(50)]).toEqual([7, 8]);
    expect([gridRows(120), columnsOf(120)]).toEqual([7, 18]);
    expect(gridRows(0)).toBe(1);
  });

  it('grid rows fewer than columns', () => {
    for (let squares = 1; squares <= 200; squares++) {
      const rows = gridRows(squares);
      const columns = columnsOf(squares);
      expect(rows >= 1 && rows <= 7).toBe(true);
      expect(rows < columns || (rows === 1 && columns === 1), `${squares} squares: ${rows} by ${columns}`).toBe(true);
    }
  });

  it('grid cells run down each column', () => {
    // 3 + 4 + 5 = 12 squares: 3 rows by 4 columns.
    const cells = gridCells([task(1, 'M'), task(2, 'L'), task(3, 'XL')]);
    expect(cells).toHaveLength(12);
    cells.forEach((cell, i) => {
      expect(cell.column).toBe(Math.floor(i / 3));
      expect(cell.row).toBe(i % 3);
    });
    expect(cells.map((c) => c.task.number)).toEqual([1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 3]);
    expect(cells.filter((c) => c.task.number === 2).map((c) => c.index)).toEqual([0, 1, 2, 3]);
    // No cell is left empty before the last square: every place of the grid is taken once.
    expect(new Set(cells.map((c) => `${c.column},${c.row}`)).size).toBe(12);
  });

  it('grid cells carry a task into the next column', () => {
    // S 2 + M 3 + XL 5 + S 2 = 12 squares, 3 rows: the M starts on column 0's last row.
    const cells = gridCells([task(1, 'S'), task(2, 'M'), task(3, 'XL'), task(4, 'S')]);
    const m = cells.filter((c) => c.task.number === 2);
    expect(m.map((c) => [c.column, c.row])).toEqual([
      [0, 2],
      [1, 0],
      [1, 1],
    ]);
  });

  it('grid cells count a task with no size as two', () => {
    expect(gridCells([task(1, undefined)])).toHaveLength(2);
    expect(gridCells([task(1, null), task(2, 'XS')])).toHaveLength(3);
    expect(gridCells([task(1, 'XS'), task(2, 'S'), task(3, 'M'), task(4, 'L'), task(5, 'XL'), task(6, 'E')])).toHaveLength(21);
    expect(gridCells([])).toEqual([]);
  });
});
