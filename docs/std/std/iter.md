# `std.iter`

Range tools that do not need a new type.

The algorithms in `std.algorithm` are whole-range operations; these are the
small moves a loop makes on a pair of positions:

    cursor *int = iter.advance(values.begin(), 2);
    stop *int = iter.skip(values.begin(), values.end(), 2);   // same thing
    first *int = iter.find_if(values.begin(), values.end(), odd);

Each is one template over the iterator type, so a pointer range and a
`Deque`'s `ListIterator`-like class both work; the `@enable_if` says which
iterator operations the body needs.

## Declarations

### `fn advance(first It, last It, count usize) It`

Moves `first` forward `count` steps, or to `last` if the range ends first.
- `first`: where the range starts
- `last`: one past the range's end
- `count`: how many steps to take

```
@enable_if(InputIterator(It))
export
template <It type>
fn advance(first It, last It, count usize) It
```

### `fn skip(first It, last It, count usize) It`

The position after skipping `count` elements from `first`.
- `first`: where the range starts
- `last`: one past the range's end
- `count`: how many elements to pass over

```
@enable_if(InputIterator(It))
export
template <It type>
fn skip(first It, last It, count usize) It
```

### `fn take(first It, last It, count usize) It`

The position after taking `count` elements from `first`.
- `first`: where the range starts
- `last`: one past the range's end
- `count`: how many elements to keep

```
@enable_if(InputIterator(It))
export
template <It type>
fn take(first It, last It, count usize) It
```

### `fn find_if(first It, last It, predicate Pred) It`

The position of the first element a predicate accepts, or `last`.
- `first`: where the range starts
- `last`: one past the range's end
- `predicate`: called with an element; the first one it accepts is the answer

```
@enable_if(InputIterator(It))
export
template <It type, Pred type>
fn find_if(first It, last It, predicate Pred) It
```

### `fn count_if(first It, last It, predicate Pred) usize`

How many elements a predicate accepts.
- `first`: where the range starts
- `last`: one past the range's end
- `predicate`: called with an element; every one it accepts is counted

```
@enable_if(InputIterator(It))
export
template <It type, Pred type>
fn count_if(first It, last It, predicate Pred) usize
```

### `fn all_of(first It, last It, predicate Pred) bool`

Whether every element satisfies a predicate.
- `first`: where the range starts
- `last`: one past the range's end
- `predicate`: called with an element; the walk stops at the first it rejects

```
@enable_if(InputIterator(It))
export
template <It type, Pred type>
fn all_of(first It, last It, predicate Pred) bool
```

### `fn any_of(first It, last It, predicate Pred) bool`

Whether any element satisfies a predicate.

- `first`: where the range starts
- `last`: one past the range's end
- `predicate`: called with an element until one accepts

```
@enable_if(InputIterator(It))
export
template <It type, Pred type>
fn any_of(first It, last It, predicate Pred) bool
```

### `fn none_of(first It, last It, predicate Pred) bool`

Whether no element satisfies a predicate.

- `first`: where the range starts
- `last`: one past the range's end
- `predicate`: called with an element until one accepts

```
@enable_if(InputIterator(It))
export
template <It type, Pred type>
fn none_of(first It, last It, predicate Pred) bool
```

### `fn nth(first It, last It, index usize) It`

The position of the `index`th element, or `last` when the range is shorter.

- `first`: where the range starts
- `last`: one past the range's end
- `index`: which element, counting from zero

```
@enable_if(InputIterator(It))
export
template <It type>
fn nth(first It, last It, index usize) It
```

### `export type Even {`

A predicate that accepts even numbers.

```
export type Even {
}
```

### `export fn __init__(this &Even) void {`

Creates the predicate.

```
export fn __init__(this &Even) void {
}
```

### `export fn __call__(this &const Even, value &const int) bool {`

Answers whether `value` is even.

```
export fn __call__(this &const Even, value &const int) bool {
    return value % 2 == 0;
}
```

### `export type Odd {`

A predicate that accepts odd numbers.

```
export type Odd {
}
```

### `export fn __init__(this &Odd) void {`

Creates the predicate.

```
export fn __init__(this &Odd) void {
}
```

### `export fn __call__(this &const Odd, value &const int) bool {`

Answers whether `value` is odd.

```
export fn __call__(this &const Odd, value &const int) bool {
    return value % 2 != 0;
}
```
