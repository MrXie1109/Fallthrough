/*
 * Stand-in runtime for the compiler's builtins.
 *
 * `__builtin_panic` is the placeholder for `std.panic`, which the language
 * specification mentions but which does not exist yet. The compiler emits calls
 * to it from debug-mode overflow and division-by-zero guards, so a stub is
 * needed to link and run programs that contain those guards.
 *
 * When `std.panic` is implemented this file goes away, and the compiler emits a
 * call to it instead.
 *
 * The `__builtin_render_*` functions are different: they are not placeholders,
 * they are how `str(x)`/`repr(x)` write down a value whose type has no
 * `__str__`. They belong here permanently.
 */
#include <stdio.h>
#include <stdlib.h>

/*
 * Render a value as text, for `str(x)` and `repr(x)`.
 *
 * These are called when the value's type declares no `__str__`/`__repr__`, so
 * the compiler has to write the value down itself. What it can write is a C
 * string; turning that into a `String` is the string type's business, and the
 * compiler does it by calling `String`'s own constructor on the result. So
 * nothing here knows what a `String` looks like.
 *
 * The buffer is static and there is one per kind, so a caller must copy out of
 * it before rendering another value of the same kind. That is enough for the
 * compiler, which renders, copies, and forgets -- but it is why these are not
 * something a program calls.
 *
 * A signed and an unsigned integer of each width, because `%d` on a `u32` whose
 * top bit is set prints a negative number; a bool renderer, because `true` is
 * not 1; and a character renderer, because a character is not a number either.
 */
#define RENDER_BUFFER 64

const char *__builtin_render_int(int value) {
    static char buffer[RENDER_BUFFER];
    snprintf(buffer, sizeof buffer, "%d", value);
    return buffer;
}

const char *__builtin_render_uint(unsigned value) {
    static char buffer[RENDER_BUFFER];
    snprintf(buffer, sizeof buffer, "%u", value);
    return buffer;
}

const char *__builtin_render_long(long long value) {
    static char buffer[RENDER_BUFFER];
    snprintf(buffer, sizeof buffer, "%lld", value);
    return buffer;
}

const char *__builtin_render_ulong(unsigned long long value) {
    static char buffer[RENDER_BUFFER];
    snprintf(buffer, sizeof buffer, "%llu", value);
    return buffer;
}

/*
 * `true` and `false`, because a `bool` written down as 1 and 0 is a number
 * pretending to be a truth value.
 */
const char *__builtin_render_bool(int value) {
    return value ? "true" : "false";
}

/*
 * `repr` of a C string: the text in quotes, so that an empty string, a space at
 * either end, and a newline inside all still read as what they are.
 */
const char *__builtin_render_repr(const char *text) {
    static char buffer[RENDER_BUFFER * 4];
    size_t length = 0;
    buffer[length++] = '"';
    for (; text != NULL && *text != '\0'; text++) {
        unsigned char byte = (unsigned char)*text;
        /* Room for an escape plus the closing quote and terminator. */
        if (length + 8 >= sizeof buffer) {
            break;
        }
        switch (byte) {
        case '"':
            buffer[length++] = '\\';
            buffer[length++] = '"';
            break;
        case '\\':
            buffer[length++] = '\\';
            buffer[length++] = '\\';
            break;
        case '\n':
            buffer[length++] = '\\';
            buffer[length++] = 'n';
            break;
        case '\t':
            buffer[length++] = '\\';
            buffer[length++] = 't';
            break;
        case '\r':
            buffer[length++] = '\\';
            buffer[length++] = 'r';
            break;
        default:
            if (byte < 0x20 || byte == 0x7F) {
                snprintf(buffer + length, sizeof buffer - length, "\\x%02X", byte);
                length += 4;
            } else {
                buffer[length++] = (char)byte;
            }
            break;
        }
    }
    buffer[length++] = '"';
    buffer[length] = '\0';
    return buffer;
}

/*
 * A character, encoded as UTF-8 from its code point. One renderer for all three
 * character widths: `c8` is the one-byte case, and `c16`/`c32` are code points
 * like any other, so encoding them the same way is what makes `str(c)` a
 * character rather than a number.
 */
static size_t encode_utf8(char *buffer, unsigned value) {
    size_t length = 0;
    if (value < 0x80) {
        buffer[length++] = (char)value;
    } else if (value < 0x800) {
        buffer[length++] = (char)(0xC0 | (value >> 6));
        buffer[length++] = (char)(0x80 | (value & 0x3F));
    } else if (value < 0x10000) {
        buffer[length++] = (char)(0xE0 | (value >> 12));
        buffer[length++] = (char)(0x80 | ((value >> 6) & 0x3F));
        buffer[length++] = (char)(0x80 | (value & 0x3F));
    } else {
        buffer[length++] = (char)(0xF0 | (value >> 18));
        buffer[length++] = (char)(0x80 | ((value >> 12) & 0x3F));
        buffer[length++] = (char)(0x80 | ((value >> 6) & 0x3F));
        buffer[length++] = (char)(0x80 | (value & 0x3F));
    }
    return length;
}

const char *__builtin_render_char(unsigned value) {
    static char buffer[8];
    size_t length = encode_utf8(buffer, value);
    buffer[length] = '\0';
    return buffer;
}

/*
 * `repr` of a character: in single quotes, with the same escapes a string gets,
 * because a character written down without them is indistinguishable from the
 * text it happens to be.
 */
const char *__builtin_render_char_repr(unsigned value) {
    static char buffer[16];
    size_t length = 0;
    buffer[length++] = '\'';
    switch (value) {
    case '\'':
        buffer[length++] = '\\';
        buffer[length++] = '\'';
        break;
    case '\\':
        buffer[length++] = '\\';
        buffer[length++] = '\\';
        break;
    case '\n':
        buffer[length++] = '\\';
        buffer[length++] = 'n';
        break;
    case '\t':
        buffer[length++] = '\\';
        buffer[length++] = 't';
        break;
    case '\r':
        buffer[length++] = '\\';
        buffer[length++] = 'r';
        break;
    default:
        if (value < 0x20 || value == 0x7F) {
            snprintf(buffer + length, sizeof buffer - length, "\\x%02X", value);
            length += 4;
        } else {
            length += encode_utf8(buffer + length, value);
        }
        break;
    }
    buffer[length++] = '\'';
    buffer[length] = '\0';
    return buffer;
}

const char *__builtin_render_float(double value) {
    static char buffer[RENDER_BUFFER];
    /*
     * `%g` rather than `%f`, so that a whole number reads as `2` instead of
     * `2.000000`. Six significant digits is what C gives by default; a
     * language-level float formatter belongs in `std.String`.
     */
    snprintf(buffer, sizeof buffer, "%g", value);
    return buffer;
}

const char *__builtin_render_pointer(const void *value) {
    static char buffer[RENDER_BUFFER];
    snprintf(buffer, sizeof buffer, "%p", value);
    return buffer;
}

void __builtin_panic(const char *message) {
    fputs("panic: ", stderr);
    fputs(message, stderr);
    fputc('\n', stderr);
    /*
     * The message is flushed before aborting, because `abort` does not flush
     * stdio buffers. Without this the text can be lost or interleaved with
     * whatever the C library prints afterwards, which is how the message ended
     * up appearing twice with no separator.
     */
    fflush(stderr);
    abort();
}
