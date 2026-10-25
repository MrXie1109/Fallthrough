# 10 Diagnostics

> [中文](10-diagnostics.zh.md)

## Scope

This volume specifies the numbering, structure, severity levels, grouping and
suppression mechanisms of the diagnostics reported by an implementation. The specific
wording of diagnostic text is not normative; numbers, locations and categories are
normative.

## Numbers

2.1 Each diagnostic carries a stable number consisting of a phase letter and four
digits.

| Prefix | Phase |
| ---- | ---- |
| `D` | driver |
| `L` | lexing and macro expansion |
| `P` | parsing |
| `S` | semantics (name resolution and type checking) |
| `I` | internal error |

2.2 A number is part of the implementation's public contract: the same error **must** use
the same number in different versions; the wording may change.

2.3 An `I` diagnostic means that the implementation failed to complete its work, not
that the program is wrong. The implementation **must** report such a diagnostic at bug
severity and identify the construct it could not handle.

## Diagnostic structure

3.1 A diagnostic contains:

- a severity level (error or warning);
- a stable number;
- a primary location: the source-text range marked with `^^^`;
- a message: one sentence stating the problem;
- an optional supplementary note (`= note:`): the relevant rule or the way to correct
  the program.

3.2 A diagnostic that involves a language rule **should** give a supplementary note for
the corresponding rule.

3.3 The location of a diagnostic **must** point to the source-text range that causes the
problem.

## Severity levels

4.1 `error` means that the program does not conform to this standard; the implementation
**must** reject the program.

4.2 `warning` means that the program may compile, but its intent **should** be examined.
A warning does not change the exit code, unless the implementation is asked to treat
warnings as errors by means such as `-Werror`.

4.3 Warnings may be controlled by group. Group names are part of the implementation
interface.

## Warning groups

| Group | What is reported | Number |
| -- | -------- | ---- |
| `unused` | a local variable that is never read after its declaration | `S0140` |
| `unused-parameter` | a parameter that is never read (except the parameters of an `extern` function and the type parameters of a trait) | `S0142` |
| `shadow` | a local declaration that shadows an outer binding in the same function | `S0143` |
| `constant-condition` | an `if`/`while` whose condition is a literal (except `while (true)`) | `S0144` |
| `assignment-in-condition` | an assignment in condition position | `S0145` |
| `self-comparison` | a value compared with itself (except `it < it` in a trait requirement and the floating-point `x != x`) | `S0146` |
| `division-by-zero` | a divisor known at compile time to be zero | `S0147` |
| `use-after-move` | reading a value after it was moved | `S0148` |
| `unreachable` | a statement that can never be executed | `S0141` |
| `useless-raise-mark` | `??` attached to a call that does not raise | `S0083` |
| `deprecated` | use of a declaration carrying `@deprecated` | `S0160` |

5.1 The implementation **must** provide a way to enable and disable warnings by group:

```text
-W<name>        report this group (all groups are reported by default)
-Wno-<name>     do not report this group
-Wall           restore the default: report every group
-Werror         treat reported warnings as errors
```

5.2 When a group name is misspelled, the implementation **must** report a usage error
and list the available group names.

5.3 `-Wverbose` **should** add the group name and an explanation to a diagnostic; the
default output does not add them.

5.4 When `-Wno-<name>` and `-Werror` are used together, a disabled group is neither
reported nor treated as an error.

5.5 A name beginning with `_` indicates deliberate non-use; `unused` and
`unused-parameter` remain silent about it.

5.6 Warnings are reported **only** for source files written by the user; code from an
imported library is not reported.

5.7 The implementation of `use-after-move` is conservative: it tracks statement order
within the same block, and moves in nested blocks are not counted.

5.8 `shadow` considers **only** names within the same function.

5.9 `constant-condition` recognises literals only.

## In-place suppression

6.1 A warning may be suppressed in place by a comment at the end of a line. Only the
following three forms constitute a directive:

```ft
x int = 1;                    // no warning
// explanatory text -- no warning
by_code int = 2;              // no warning: S0140
by_group int = 3;             // no warning: unused
```

6.2 A comment whose content is exactly `no warning` suppresses every warning on that
line.

6.3 A comment ending with `-- no warning`: the same; the text before `--` is an
explanation.

6.4 `no warning: NAME[, NAME]…` suppresses only the specified numbers or groups.

6.5 A comment that merely mentions `no warning` is not a directive.

6.6 Where a directive takes effect: the reported line, a comment-only line above it, or
any line covered by the diagnostic.

6.7 A directive **must** be at the end of a line; text inside a string literal does not
constitute a directive.

6.8 In-place suppression takes precedence over `-Werror` and applies only to warnings.

## Color

7.1 The color of diagnostics is controlled by implementation options: `auto` (the
default, coloring only a terminal), `always`, `never`.

7.2 When the environment variable `NO_COLOR` is set, `auto` does not color.

7.3 The diagnostic text with escape sequences removed **must** be byte-for-byte
identical to the text when uncolored.

## Deduplication

8.1 The implementation **must** deduplicate diagnostics whose severity level, number,
primary location and message are all the same.

8.2 The same error occurring in multiple instantiations of the same template
**should** be reported only once.

8.3 Diagnostics with different locations or different messages **must not** be merged.

## Entries not yet finalised

9.1 Entries marked *Open* in [`../SPEC-STATUS.md`](../SPEC-STATUS.md) are not behavior
specified by this standard.

9.2 On encountering such an entry, an implementation **must** report `I0999` and must not
guess at a semantics.

## Code reference

Every stable code, its severity and message template is in
[Appendix A](appendix-a-errors.md).
