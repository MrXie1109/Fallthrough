# `std.regex`

Regular expressions: a small backtracking matcher.

    expression Regex = regex.compile(&String("^a[0-9]+z$"))??;
    found Match = expression.find(&text).value()??;

# The syntax

The part of regular expressions a command parser or a log filter uses:

* a literal character matches itself;
* `.` matches any character except a newline;
* `[abc]`, `[a-z]`, `[^0-9]` match one character from (or not from) a set;
* `\d` (digit), `\w` (word character), `\s` (space), `\.` (a literal dot and
  the other punctuation escapes);
* `^` and `$` anchor to the start and the end of the text;
* `*`, `+` and `?` repeat the preceding atom, greedily.

Out of scope, and refused at `compile` time as syntax errors: groups,
alternation, back-references, lazy quantifiers and bounded repetition. A
matcher that says what it does not support is more useful than one that
silently reads `(` as a literal.

# Matching

The matcher tries each start position and then matches the atoms in order,
backtracking when a repetition was too greedy. That is exponential in the
worst case -- a pattern like `a*a*a*b` on many `a`s -- and it is stated here
because it is fine for configuration-sized texts and not for a hostile one.

## Declarations

### `export type Match {`

The result of a search: a half-open range of the text.

```
export type Match {
    // The index of the first matched character.
    _start usize;
    // The index one past the last matched character.
    _end usize;
}
```

### `export fn start(this &const Match) usize {`

The start of the match.

```
export fn start(this &const Match) usize {
    return this._start;
}
```

### `export fn end(this &const Match) usize {`

One past the end of the match.

```
export fn end(this &const Match) usize {
    return this._end;
}
```

### `export fn length(this &const Match) usize {`

The length of the match.

```
export fn length(this &const Match) usize {
    return this._end - this._start;
}
```

### `export fn matched(this &const Match, text &const String) String except {`

The matched part of `text`.

```
export fn matched(this &const Match, text &const String) String except {
    if (this._end > text.length()) {
        raise "ValueError": "the match does not belong to this text";
    }
    return text.substring(this._start, this._end - this._start);
}
```

### `export type Regex {`

A compiled pattern.

```
export type Regex {
    // The pattern text.
    _pattern String;
}
```

### `export fn compile(pattern &const String) Regex except {`

Compiles `pattern`, refusing syntax this matcher does not implement.

```
export fn compile(pattern &const String) Regex except {
    index usize = 0;
    while (index < pattern.length()) {
        character char = pattern[index];
        if (character == '(' || character == ')' || character == '|') {
            raise "ValueError": "groups and alternation are not supported";
        }
        if (character == '{' || character == '}') {
            raise "ValueError": "bounded repetition is not supported";
        }
        if (character == '\\' || character == '[') {
            // The escapes and classes are validated by the matcher's atom
            // scanner; that keeps one place that understands their length.
            index = atom_end(pattern, index)??;
        } else {
            index += 1;
        }
    }
    expression Regex = .{ String(pattern) };
    return expression;
}
```

### `fn atom_end(pattern &const String, index usize) usize except`

The index just past the atom starting at `index`.
- `pattern`: the pattern text being compiled
- `index`: the byte offset of the atom's first character, counted from
  zero

```
fn atom_end(pattern &const String, index usize) usize except
```

### `fn atom_matches(`

Whether the atom between `start` and `stop` matches `character`.
- `pattern`: the pattern the atom belongs to
- `start`: the byte offset of the atom's first character, counted from
  zero
- `stop`: one past the atom's last character, the quantifier included,
  so it is exclusive
- `character`: the one text byte the atom is tried against

```
fn atom_matches(
    pattern &const String,
    start usize,
    stop usize,
    character char
) bool
```

### `fn class_matches(`

Whether `character` is in the class between `start` and `stop`.
- `pattern`: the pattern holding the class, its `[` and `]` included
- `start`: the byte offset just past the `[`, or past the `^` when the
  class is negated
- `stop`: the byte offset of the closing `]`, so it is exclusive
- `character`: the one text byte the class is tried against

```
fn class_matches(
    pattern &const String,
    start usize,
    stop usize,
    character char
) bool
```

### `fn is_digit(character char) bool`

Whether `character` is a digit.
- `character`: the byte to test; only the ASCII digits count

```
fn is_digit(character char) bool
```

### `fn is_word(character char) bool`

Whether `character` is a word character.
- `character`: the byte to test; only ASCII letters, digits and `_` count

```
fn is_word(character char) bool
```

### `fn is_space(character char) bool`

Whether `character` is whitespace.
- `character`: the byte to test; space, tab, newline and carriage return
  count, and nothing else does

```
fn is_space(character char) bool
```

### `fn match_sequence(`

Matches the pattern from `pattern_index` against `text` from `text_index`.

Returns the end of the match when the rest succeeds, or none.
- `pattern`: the whole pattern, which is read from `pattern_index` on
- `pattern_index`: the byte offset in the pattern to match from, counted from
  zero
- `text`: the text being searched
- `text_index`: the byte offset in the text the pattern is tried at, counted
  from zero
- `must_end`: whether the match must reach the end of the text, which is
  what `$` needs and a plain search does not

```
fn match_sequence(
    pattern &const String,
    pattern_index usize,
    text &const String,
    text_index usize,
    must_end bool
) ?usize except
```

### `export fn find(this &const Regex, text &const String) ?Match except {`

Searches `text` for the first match.

```
export fn find(this &const Regex, text &const String) ?Match except {
    anchored bool = this._pattern.length() > 0usize && this._pattern[0] == '^';
    start usize = 0usize;
    while (start <= text.length()) {
        found ?usize = match_sequence(this._pattern, 0usize, text, start, false)??;
        if (found.present()) {
            end_index usize = found.value()??;
            return optional.some<Match>(.{ start, end_index });
        }
        if (anchored) {
            return optional.none<Match>();
        }
        start += 1usize;
    }
    return optional.none<Match>();
}
```

### `export fn is_match(this &const Regex, text &const String) bool except {`

Whether `text` contains a match.

```
export fn is_match(this &const Regex, text &const String) bool except {
    found ?Match = this.find(text)??;
    return found.present();
}
```

### `export fn pattern(this &const Regex) String {`

The pattern text.

```
export fn pattern(this &const Regex) String {
    return this._pattern;
}
```
