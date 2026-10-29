# `std.grid`

A two-dimensional array: rows of equal length, one buffer.

    grid Grid<int> = Grid<int>(2usize, 3usize, 0);
    grid.set(1, 2, 7);
    value int = grid.at(1, 2);
    rows usize = grid.rows();

The storage is one `Array<T>` in row-major order: element `(row, column)` is
at `row * columns + column`. That is the layout a matrix and an image share,
so a grid of numbers is a contiguous run of numbers and a row is a slice of
it.

# A filler value

A grid has no way to default-construct its elements -- the element type may
have no such constructor -- so the constructor takes a filler value that every
cell starts as. `Grid<int>(rows, columns, 0)` is a grid of zeros.

## Declarations

### `type Grid {`

A two-dimensional array of `T`.

```
export
template <T type>
type Grid {
    // The cells, row by row.
    _data Array<T>;
    // How many rows.
    _rows usize;
    // How many columns each row has.
    _columns usize;
}
```

### `fn __init__(this &Grid<T>, rows usize, columns usize, filler T) void`

Creates a grid of `rows` by `columns` cells, each a copy of `filler`.

- `rows`: how many rows the grid has
- `columns`: how many columns each row has
- `filler`: the value every cell starts as

```
export
template <T type>
fn __init__(this &Grid<T>, rows usize, columns usize, filler T) void
```

### `fn __init__(this &Grid<T>, other &const Grid<T>) void`

Copies another grid.

- `other`: the grid to copy

```
export
template <T type>
fn __init__(this &Grid<T>, other &const Grid<T>) void
```

### `fn __drop__(this &Grid<T>) void`

Releases the cells.

```
export
template <T type>
fn __drop__(this &Grid<T>) void
```

### `fn rows(this &const Grid<T>) usize`

How many rows.

```
export
template <T type>
fn rows(this &const Grid<T>) usize
```

### `fn columns(this &const Grid<T>) usize`

How many columns.

```
export
template <T type>
fn columns(this &const Grid<T>) usize
```

### `fn length(this &const Grid<T>) usize`

How many cells.

```
export
template <T type>
fn length(this &const Grid<T>) usize
```

### `fn empty(this &const Grid<T>) bool`

Whether there are no cells.

```
export
template <T type>
fn empty(this &const Grid<T>) bool
```

### `fn at(this &Grid<T>, row usize, column usize) &T`

The cell at `(row, column)`; writing through it writes the grid.

- `row`: the row, counting from zero
- `column`: the column, counting from zero

```
export
template <T type>
fn at(this &Grid<T>, row usize, column usize) &T
```

### `fn at(this &const Grid<T>, row usize, column usize) &const T`

The cell at `(row, column)`, read only.

- `row`: the row, counting from zero
- `column`: the column, counting from zero

```
export
template <T type>
fn at(this &const Grid<T>, row usize, column usize) &const T
```

### `fn set(this &Grid<T>, row usize, column usize, value T) void`

Stores `value` at `(row, column)`.

- `row`: the row, counting from zero
- `column`: the column, counting from zero
- `value`: what to store

```
export
template <T type>
fn set(this &Grid<T>, row usize, column usize, value T) void
```

### `fn fill(this &Grid<T>, value T) void`

Writes `value` into every cell.

- `value`: what every cell becomes

```
export
template <T type>
fn fill(this &Grid<T>, value T) void
```

### `fn __eq__(this &const Grid<T>, other &const Grid<T>) bool`

Whether two grids have the same shape and equal cells.

- `other`: the grid to compare with

```
export
template <T type>
fn __eq__(this &const Grid<T>, other &const Grid<T>) bool
```
