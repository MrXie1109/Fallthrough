# `std.term`

Colour and text styles for a terminal.

    std.print("{}{}warning{}\n", term.bold(), term.yellow(), term.reset())??;

The escape sequences are ANSI SGR, which every terminal on the platforms here
understands. The *policy* -- whether to emit them at all -- is
`supports_color()`: a `NO_COLOR` environment variable, a dumb terminal, or an
output that is not a terminal all say no, so a program can colour
unconditionally and still be readable when redirected to a file.

## Declarations

### `export enum Color int {`

The eight colours every terminal has.

```
export enum Color int {
    // Black.
    BLACK,
    // Red.
    RED,
    // Green.
    GREEN,
    // Yellow.
    YELLOW,
    // Blue.
    BLUE,
    // Magenta.
    MAGENTA,
    // Cyan.
    CYAN,
    // White.
    WHITE,
}
```

### `fn code(color Color) int`

The SGR number of a colour.

- `color`: the colour to name

```
fn code(color Color) int
```

### `export fn color_fg(color Color) String {`

The escape sequence for `color` as the foreground.

```
export fn color_fg(color Color) String {
    return sgr(code(color));
}
```

### `export fn color_bg(color Color) String {`

The escape sequence for `color` as the background.

```
export fn color_bg(color Color) String {
    return sgr(code(color) + 10);
}
```

### `export fn style_bold() String {`

Bold text.

```
export fn style_bold() String {
    return sgr(1);
}
```

### `export fn style_dim() String {`

Dim text.

```
export fn style_dim() String {
    return sgr(2);
}
```

### `export fn style_underline() String {`

Underlined text.

```
export fn style_underline() String {
    return sgr(4);
}
```

### `export fn style_reset() String {`

Normal intensity and no underline.

```
export fn style_reset() String {
    return sgr(0);
}
```

### `fn sgr(number int) String`

Builds one SGR sequence.

- `number`: the SGR parameter to write between the escape and its `m`

```
fn sgr(number int) String
```

### `export fn supports_color() bool {`

Whether colour is appropriate on standard output.

Three answers are no: `NO_COLOR` is set (the convention for "I do not want
colour"), `TERM` is `dumb`, and standard output is not a terminal.

```
export fn supports_color() bool {
    no_color String = String("NO_COLOR");
    if (os.hasenv(no_color.data())) {
        return false;
    }
    term_text String = String("TERM");
    setting String = String();
    try {
        setting = os.getenv(term_text.data())??;
    } except {
        // An environment that cannot be read is not a reason to refuse colour;
        // the terminal check below still answers.
    }
    dumb String = String("dumb");
    if (setting == dumb) {
        return false;
    }
    return term_sys.is_terminal(1);
}
```
