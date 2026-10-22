# The C bindings

> [中文](c-bindings.zh.md)

`standard/C.ftm` is an **ordinary module**: it declares nothing itself and re-exports one
submodule per C header. So after `import C;` the names `C.printf`, `C.sqrt` and `C.malloc`
are available -- they are C's functions, declared with C's ABI.

```ft
import C;

fn main() int {
    C.printf("%d\n", C.abs(-3));
    return 0;
}
```

- A name is C's name; `C.math.sin` works too (`C.math` is `standard/C/math.ftm`).
- `FILE *` is written `*c8` here: it is C's storage, and a program hands it back without
  looking inside it.
- `va_list`, function-pointer parameters, and a few C facilities that only exist as macros
  cannot be written honestly, so the functions that need them are not declared.
- Two type correspondences the declarations rest on are "a C `char *` is `*const c8`" and
  "a C `size_t` is `usize`"; both assumptions are stated in `standard/C.ftm`.

The tables below are taken from `standard/C/*.ftm`: a signature is the declaration the
module writes, and the purpose is the first sentence of the comment above it.


## `C.assert`

| Name | Signature | Purpose (from the module's own comments) |
| ---- | ---- | ---- |
| `__assert_fail` | `__assert_fail(assertion *const c8, file *const c8, line u32, function *const c8) void` | Aborts with a message if the condition is zero. |

## `C.ctype`

| Name | Signature | Purpose (from the module's own comments) |
| ---- | ---- | ---- |
| `isalpha` | `isalpha(character int) int` | Reports whether a character is alphabetic. |
| `isdigit` | `isdigit(character int) int` | Reports whether a character is a digit. |
| `isalnum` | `isalnum(character int) int` | Reports whether a character is alphanumeric. |
| `isspace` | `isspace(character int) int` | Reports whether a character is whitespace. |
| `isupper` | `isupper(character int) int` | Reports whether a character is uppercase. |
| `islower` | `islower(character int) int` | Reports whether a character is lowercase. |
| `isprint` | `isprint(character int) int` | Reports whether a character is printable. |
| `toupper` | `toupper(character int) int` | Converts a character to uppercase. |
| `tolower` | `tolower(character int) int` | Converts a character to lowercase. |

## `C.math`

| Name | Signature | Purpose (from the module's own comments) |
| ---- | ---- | ---- |
| `fabs` | `fabs(value double) double` | The absolute value of a double. |
| `sqrt` | `sqrt(value double) double` | The square root. |
| `pow` | `pow(base double, exponent double) double` | Raises to a power. |
| `exp` | `exp(value double) double` | The exponential function. |
| `log` | `log(value double) double` | The natural logarithm. |
| `log10` | `log10(value double) double` | The base-10 logarithm. |
| `sin` | `sin(value double) double` | The sine. |
| `cos` | `cos(value double) double` | The cosine. |
| `tan` | `tan(value double) double` | The tangent. |
| `floor` | `floor(value double) double` | The floor. |
| `ceil` | `ceil(value double) double` | The ceiling. |
| `round` | `round(value double) double` | Rounds to the nearest integer. |
| `fmod` | `fmod(numerator double, denominator double) double` | The floating-point remainder. |

## `C.stdio`

| Name | Signature | Purpose (from the module's own comments) |
| ---- | ---- | ---- |
| `stdin` | `stdin *c8` | The standard streams, which the C library owns. |
| `stdout` | `stdout *c8` | — |
| `stderr` | `stderr *c8` | — |
| `EOF` | `EOF const int = -1` | The value `getchar` and friends return at end of input. |
| `printf` | `printf(fmt *const c8, ...) int` | Writes formatted output to stdout. |
| `fprintf` | `fprintf(stream *c8, fmt *const c8, ...) int` | Writes formatted output to a stream. |
| `sprintf` | `sprintf(buffer *c8, fmt *const c8, ...) int` | Writes formatted output into a buffer. |
| `snprintf` | `snprintf(buffer *c8, size usize, fmt *const c8, ...) int` | As `sprintf`, with a size limit. |
| `puts` | `puts(text *const c8) int` | Writes a string and a newline to stdout. |
| `fputs` | `fputs(text *const c8, stream *c8) int` | Writes a string to a stream. |
| `fputc` | `fputc(character int, stream *c8) int` | Writes one character to a stream. |
| `putchar` | `putchar(character int) int` | Writes one character to stdout. |
| `scanf` | `scanf(fmt *const c8, ...) int` | Reads formatted input from stdin. |
| `fgets` | `fgets(buffer *c8, size int, stream *c8) *c8` | Reads a line from a stream into a buffer. |
| `fgetc` | `fgetc(stream *c8) int` | Reads one character from a stream. |
| `getchar` | `getchar() int` | Reads one character from stdin. |
| `fopen` | `fopen(path *const c8, mode *const c8) *c8` | Opens a file. |
| `fclose` | `fclose(stream *c8) int` | Closes a file. |
| `fflush` | `fflush(stream *c8) int` | Flushes a stream's buffer. |
| `fread` | `fread(buffer *c8, size usize, count usize, stream *c8) usize` | Reads a whole file into a buffer. |
| `fwrite` | `fwrite(buffer *const c8, size usize, count usize, stream *c8) usize` | Writes to a file. |
| `fseek` | `fseek(stream *c8, offset isize, whence int) int` | Moves a stream's position. |
| `ftell` | `ftell(stream *c8) isize` | Reports a stream's position. |
| `rewind` | `rewind(stream *c8) void` | Rewinds a stream to its start. |
| `feof` | `feof(stream *c8) int` | Reports the end-of-file indicator. |
| `ferror` | `ferror(stream *c8) int` | Reports a stream error. |
| `strerror` | `strerror(code int) *c8` | Reports the last error as a string. |

## `C.stdlib`

| Name | Signature | Purpose (from the module's own comments) |
| ---- | ---- | ---- |
| `EXIT_SUCCESS` | `EXIT_SUCCESS const int = 0` | The two values `exit` takes. |
| `EXIT_FAILURE` | `EXIT_FAILURE const int = 1` | — |
| `malloc` | `malloc(size usize) *byte` | Allocates uninitialised memory. |
| `calloc` | `calloc(count usize, size usize) *byte` | Allocates zeroed memory. |
| `realloc` | `realloc(pointer *byte, size usize) *byte` | Resizes an allocation. |
| `free` | `free(pointer *byte) void` | Releases an allocation. |
| `atoi` | `atoi(text *const c8) int` | Parses an integer. |
| `atol` | `atol(text *const c8) i64` | Parses a long integer. |
| `atof` | `atof(text *const c8) double` | Parses a floating-point number. |
| `strtol` | `strtol(text *const c8, end *c8, base int) i64` | Parses an integer, reporting where parsing stopped. |
| `strtod` | `strtod(text *const c8, end *c8) double` | Parses a floating-point number, reporting where parsing stopped. |
| `exit` | `exit(status int) void` | Exits the process. |
| `abort` | `abort() void` | Aborts the process. |
| `abs` | `abs(value int) int` | Returns the absolute value of an integer. |
| `labs` | `labs(value i64) i64` | Returns the absolute value of a long integer. |
| `div` | `div(numerator int, denominator int) i64` | Divides two integers. |
| `rand` | `rand() int` | Generates a pseudo-random number. |
| `srand` | `srand(seed u32) void` | Seeds the pseudo-random generator. |
| `qsort` | `qsort(base *byte, count usize, size usize, compare *byte) void` | Sorts an array in place. |
| `bsearch` | `bsearch(key *const byte, base *const byte, count usize, size usize, compare *byte) *byte` | Searches a sorted array. |
| `system` | `system(command *const c8) int` | Runs a command through the shell. |
| `getenv` | `getenv(name *const c8) *c8` | Reads an environment variable. |

## `C.string`

| Name | Signature | Purpose (from the module's own comments) |
| ---- | ---- | ---- |
| `strlen` | `strlen(text *const c8) usize` | Returns the length of a string, not counting the terminator. |
| `strcpy` | `strcpy(destination *c8, source *const c8) *c8` | Copies a string. |
| `strncpy` | `strncpy(destination *c8, source *const c8, size usize) *c8` | Copies at most `size` bytes of a string. |
| `strcat` | `strcat(destination *c8, source *const c8) *c8` | Appends a string. |
| `strncat` | `strncat(destination *c8, source *const c8, size usize) *c8` | Appends at most `size` bytes of a string. |
| `strcmp` | `strcmp(left *const c8, right *const c8) int` | Compares two strings. |
| `strncmp` | `strncmp(left *const c8, right *const c8, size usize) int` | Compares at most `size` bytes of two strings. |
| `strchr` | `strchr(text *const c8, character int) *c8` | Finds a character in a string. |
| `strrchr` | `strrchr(text *const c8, character int) *c8` | Finds the last occurrence of a character in a string. |
| `strstr` | `strstr(haystack *const c8, needle *const c8) *c8` | Finds a substring. |
| `memcpy` | `memcpy(destination *byte, source *const byte, size usize) *byte` | Copies a block of memory, which must not overlap. |
| `memmove` | `memmove(destination *byte, source *const byte, size usize) *byte` | Copies a block of memory, which may overlap. |
| `memset` | `memset(destination *byte, value int, size usize) *byte` | Fills a block of memory. |
| `memcmp` | `memcmp(left *const byte, right *const byte, size usize) int` | Compares two blocks of memory. |
| `memchr` | `memchr(block *const c8, value int, size usize) *c8` | Finds a byte in a block of memory. |
