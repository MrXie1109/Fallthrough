# 附录 A 诊断对照表

> [English](appendix-a-errors.md)

## A.1 范围

本附录列出本实现所提供的稳定诊断编号、严重级别与消息模板，供工具、测试与文档
引用。编号是契约：措辞可以变化，编号不随版本变化；程序与脚本**应**按编号而不是
按消息文本判断。

## A.2 编号的分配

编号由阶段字母与四位十进制数字组成：

| 前缀 | 阶段 |
| ---- | ---- |
| `D` | 驱动程序与文件、链接 |
| `L` | 词法分析与宏展开 |
| `P` | 语法分析 |
| `S` | 语义分析（名字解析、类型检查、约束检查） |
| `I` | 实现内部错误 |

消息中的 `{}` 是占位符，由实现填入具体名称、类型或数值；消息模板本身不是可依赖的
接口。同一编号在语义上表示同一类错误；一个编号可能有多条消息模板（不同上下文给出
更具体的原因）。

## A.3 错误与警告

表中「级别」为 `error` 时，程序不符合本规范，实现**必须**拒绝该程序；为 `warning`
时，程序可以编译，但**应**检查其意图（见 [10 诊断](10-diagnostics.zh.md)）。

| 编号 | 级别 | 消息模板 |
| ---- | ---- | -------- |
| `D0001` | error | cannot read `{}` |
| `D0001` | error | cannot read `{}`: {error} |
| `D0002` | error | cannot emit output: {message} |
| `D0003` | error | cannot write `{}`: {error} |
| `D0004` | error | cannot write the requested output |
| `D0005` | error | cannot find module `{name}` |
| `D0006` | error | two files provide the module `{name}` |
| `D0007` | error | two modules are reachable as `{last}` |
| `I0002` | error | generated LLVM IR failed verification |
| `I0999` | error | the code generator does not handle this construct yet |
| `L0001` | error | unrecognised character |
| `L0014` | error | this {what} is never closed |
| `P0001` | error | expected {wanted}, found {found} |
| `P0001` | error | unknown thing |
| `P0002` | error | `@align` needs an argument: `@align(16)` |
| `P0002` | error | `@{other}` is not an attribute of a field |
| `P0002` | error | `@{}` is not an attribute of this branch |
| `P0002` | error | `@{}` takes no arguments |
| `P0002` | error | `alignas` is written twice for this binding |
| `P0002` | error | `alignas` is written twice for this field |
| `P0002` | error | `except` here is the query, so it needs `(expr)` |
| `P0002` | error | `export` applies to a module-level declaration, not a local |
| `P0002` | error | `export` is a keyword, not an attribute |
| `P0002` | error | `mutable` is a property of a field, not of a binding |
| `P0002` | error | `static` is written twice for this binding |
| `P0002` | error | `thread_local` is written twice for this binding |
| `P0002` | error | a `static_assert` message must be a string literal |
| `P0002` | error | a constant is declared `NAME const Type = value;` |
| `P0002` | error | a function declaration must write its return type |
| `P0002` | error | a function type carries `except` without a condition |
| `P0002` | error | a lambda needs a body: `-> expr` or `{ .. }` |
| `P0002` | error | a method's parameter is written as its type |
| `P0002` | error | a positional argument cannot follow a named one |
| `P0002` | error | a positional element cannot follow a named one |
| `P0002` | error | a template parameter after a defaulted one must also have a default |
| `P0002` | error | a template parameter pack cannot have a default |
| `P0002` | error | a template parameter pack must be the last parameter |
| `P0002` | error | an interface parameter is a type parameter; write `it Name` |
| `P0002` | error | an interface states a method by calling it: write `it.next() -> int` |
| `P0002` | error | expected an attribute name after `@` |
| `P0002` | error | expected {wanted}, found {found} |
| `P0003` | error | expression nests too deeply |
| `P0004` | error | a parameter without a default cannot follow one with a default |
| `P0005` | error | a parameter pack must be the last parameter |
| `P0006` | error | a parameter pack cannot have a default argument |
| `S0001` | error | `{name}` is defined more than once |
| `S0002` | error | `{base}` has no method `{}` |
| `S0002` | error | `{}` has no method `{}` |
| `S0002` | error | cannot find `{name}` in this scope |
| `S0002` | error | cannot find type `{}` |
| `S0003` | error | cannot find module `{module}` |
| `S0004` | error | module `{module}` does not export `{member}` |
| `S0004` | error | module `{module}` does not export `{}` |
| `S0004` | error | module `{}` does not export `{}` |
| `S0010` | error | type mismatch in {context}: expected `{}`, found `{}` |
| `S0010` | error | type mismatch: expected {}, found {} |
| `S0011` | error | expected a value of type `{}` in this `return` |
| `S0020` | error | `[N]auto` cannot be deduced from `{}`, which is not an array |
| `S0020` | error | cannot deduce a type for `{}` from a designated initialiser |
| `S0020` | error | cannot deduce a type for `{}`: `auto` needs an initialiser |
| `S0030` | error | cannot convert `{}` to `{}` |
| `S0031` | error | `{}` cannot be applied to `{}` |
| `S0032` | error | `+` cannot add two pointers |
| `S0032` | error | `-` cannot take the difference of `{}` and `{}` |
| `S0032` | error | `<=>` cannot compare `{}` with `{}` |
| `S0032` | error | `{}=` offsets `{}` by `usize`, but `{}` was given |
| `S0032` | error | `{}` cannot compare `{}` with `{}` |
| `S0032` | error | `{}` is offset by `usize`, but `{}` was given |
| `S0032` | error | `{}` needs operands of the same type, but the left is `{}` and the right is `{}` |
| `S0033` | error | `{}=` cannot be applied to `{}` |
| `S0033` | error | `{}` cannot be applied to `{}` |
| `S0034` | error | this call returns a value, not a place, so it cannot be assigned to |
| `S0034` | error | this expression cannot be assigned to |
| `S0034` | error | this expression has no address |
| `S0035` | error | `{qualified}` is not a function |
| `S0035` | error | `{}` cannot be called with {} argument(s) |
| `S0035` | error | `{}` cannot be called |
| `S0036` | error | `{name}` takes 1 argument, but {} were given |
| `S0036` | error | `{}` has no constructor taking {} argument(s) |
| `S0036` | error | `{}` is called with {} argument(s), but `{}` takes {} |
| `S0036` | error | `{}` is given {} element(s), but `{}` supplies {} |
| `S0036` | error | `{}` takes at least {required} argument(s), but {given} were given |
| `S0036` | error | `{}` takes {declared} argument(s), but {given} were given |
| `S0036` | error | `{}` takes {expected} argument(s), but {given} were given |
| `S0036` | error | `{}` takes {} argument(s), but {} were given |
| `S0036` | error | no overload of `{name}` matches {} argument(s) |
| `S0036` | error | no overload of `{}` matches {} argument(s) |
| `S0036` | error | this lambda takes {} argument(s), but {given} were given |
| `S0037` | error | the call to `{name}` is ambiguous: {} overloads match |
| `S0040` | error | an array length cannot be negative |
| `S0040` | error | an array length must be a constant integer |
| `S0040` | error | an array length must be a constant |
| `S0041` | error | `@constexpr` binding `{}` must be initialised |
| `S0041` | error | `const` binding `{}` must be initialised |
| `S0042` | error | cannot assign to `const` binding `{name}` |
| `S0043` | error | `*` cannot be applied to `{}`, which is a reference |
| `S0043` | error | cannot dereference `{}`, which is not a pointer |
| `S0044` | error | cannot assign through a pointer to const `{}` |
| `S0044` | error | cannot assign through a pointer to const |
| `S0044` | error | cannot assign through a reference to const |
| `S0044` | error | cannot assign to `{}` of a `const` object |
| `S0044` | error | cannot assign to `{}` through a pointer to const |
| `S0050` | error | `{suffix}` cannot be the type of an integer literal |
| `S0050` | error | `{suffix}` is not a type |
| `S0051` | error | the character `{}` does not fit in `{}`; write it as `{needed}{}` |
| `S0060` | error | `{}` has no fields, so `{}` cannot be accessed |
| `S0060` | error | `{}` resolves `->` to itself |
| `S0060` | error | `{}` resolves `->` to itself, so `{}` cannot be reached |
| `S0061` | error | `{}` has no field `{}` |
| `S0062` | error | `{}` is not a member function: its first parameter is not called `this` |
| `S0063` | error | `__init__` stores a `{}`, but this slot holds a `{}` |
| `S0063` | error | `{}` cannot be called on a `const` object, because its `this` is `{}` |
| `S0063` | error | `{}` expects `this` to be `{}`, but `{}` was given |
| `S0063` | error | `{}` is a member function, so its `this` must be a pointer |
| `S0064` | error | `{{ .. }}` initialises a declared type, but `{}` is not one |
| `S0064` | error | a nested `.{ .. }` needs a type from its context |
| `S0065` | error | `{owner}` has {} field(s), but {} value(s) were given |
| `S0065` | error | field `{field_name}` of `{owner}` is `{}`, but `{}` was given |
| `S0066` | error | `{}` cannot be indexed |
| `S0066` | error | `{}` is indexed by `usize`, but `{}` was given |
| `S0066` | error | `{}` is indexed by `{}`, but `{}` was given |
| `S0066` | error | an index must be an integer, but this is `{}` |
| `S0067` | error | `{}` cannot be used as a condition |
| `S0068` | error | the value of `@constexpr` binding `{}` is not a constant expression |
| `S0068` | error | the value of `static` binding `{}` is not a constant expression |
| `S0068` | error | the value of `{}.{}` is not a constant expression |
| `S0068` | error | the value of `{}` is not a constant expression |
| `S0069` | error | `{}` of `{}` is not a constant |
| `S0070` | error | the branches of `?:` have different types: `{}` and `{}` |
| `S0071` | error | `...` after an argument needs a parameter pack to expand |
| `S0071` | error | `{}...` expands something that is not a template parameter pack |
| `S0071` | error | a comma fold needs a parameter pack to repeat |
| `S0071` | error | cannot deduce `{}` for `{}` |
| `S0071` | error | internal error: `{}` was instantiated with a pack and no elements |
| `S0071` | error | the template argument `{path}` is not a constant |
| `S0072` | error | `{}` takes {expected} template argument(s), but {given} were given |
| `S0072` | error | template parameter `{}` has no value |
| `S0073` | error | template parameter `{}` is a type, but a value was given |
| `S0073` | error | template parameter `{}` is a value, but a type was given |
| `S0073` | error | the default for template parameter `{}` is not a constant |
| `S0074` | error | the value for `{}` is not a constant |
| `S0076` | error | an enum's base type must be an integer, but `{}` was given |
| `S0078` | error | `{}.{}` is declared more than once |
| `S0079` | error | `{object_name}` has no member `{}` |
| `S0080` | error | `{name}[{index}]` is out of range: the pack has {} element(s) |
| `S0080` | error | `{name}` is a pack of types, so it has no value to index |
| `S0080` | error | `{}` is an empty pack, so folding it has no value |
| `S0080` | error | `{}` is not a template parameter pack, so it cannot be {wanted} |
| `S0080` | error | `{}` must be {wanted}, but its elements are text |
| `S0080` | error | `{}` must be {wanted}, but its elements are types (here `{}`) |
| `S0082` | error | `raise;` outside an `except` block |
| `S0082` | error | `raise` needs a type, or nothing at all to re-raise |
| `S0082` | error | a `raise` operand must be a string, but this is `{}` |
| `S0082` | error | {what} is only allowed in a function marked `except` |
| `S0083` | error | a call to `{label}` may raise, and nothing here handles it |
| `S0083` | warning | this call cannot raise, so `??` does nothing |
| `S0084` | error | `type(expr)` cannot be used here |
| `S0085` | error | `{}` contains itself by value, so it has no size |
| `S0130` | error | `{trait_name}` asks about a type, but this argument is a value |
| `S0131` | error | cannot find a trait named `{trait_name}` |
| `S0132` | error | `MoveCanRaise` is about 1 type, but {} were given |
| `S0132` | error | `{name}` is about {expected} type(s), but {} were given |
| `S0132` | error | `{name}` is about {fixed} type(s), but {} were given |
| `S0132` | error | `{trait_name}` is about {expected} type(s), but {found} were given |
| `S0135` | error | a type is not a value |
| `S0136` | error | `{name}` has no type named `{}` |
| `S0136` | error | `{}` is not a declared type, so it has no members |
| `S0137` | error | an `alignas` needs a value the compiler knows |
| `S0138` | error | `@align({value})` is not a power of two |
| `S0138` | error | `alignas({value})` is not a power of two |
| `S0139` | error | `@align({value})` is weaker than `{name}`'s own alignment of {natural} byte(s) |
| `S0139` | error | `alignas({value})` is weaker than `{name}`'s own alignment of {natural} byte(s) |
| `S0140` | warning | something to see |
| `S0141` | warning | this statement can never run |
| `S0143` | warning | `{name}` shadows an outer binding |
| `S0144` | warning | this loop can never run |
| `S0145` | warning | this condition assigns |
| `S0146` | warning | `{}` compares a value with itself |
| `S0147` | warning | `{}` divides by a constant zero |
| `S0148` | warning | `{name}` is read after it was moved from |
| `S0156` | error | a `@noreturn` function has no value to return |
| `S0157` | error | `@align` applies to a variable, not to a function |
| `S0157` | error | `@{}` applies to a function, not to a variable |
| `S0158` | error | `@{name}` is not an attribute |
| `S0159` | error | `@{}` takes {expected} argument(s), but {} were given |
| `S0160` | warning | （实现内部文本） |
| `S0162` | error | `{value}` is already a label of this `switch` |
| `S0163` | error | a `static_assert` condition must be a `bool`, but this is `{}` |
| `S0164` | error | a `static_assert` condition must be a constant |
| `S0165` | error | （实现内部文本） |
| `S0166` | error | `{}` of `{}` is not known: the type is incomplete |
| `S0167` | error | `{}` is incomplete, so its members are not known yet |
| `S0167` | error | {what} needs a complete type, and `{name}` is only declared |
| `S0168` | error | trait `{}` is incomplete, so it cannot be asked about a type yet |
| `S0169` | error | （实现内部文本） |
| `S0170` | error | `{callee}` has no parameter named `{}` |
| `S0170` | error | `{owner}` has no field named `{}` |
| `S0170` | error | `{}` fills a parameter pack, so its arguments cannot be named |
| `S0171` | error | `{callee}` is given `{}` twice |
| `S0171` | error | field `{}` of `{owner}` is given twice |
| `S0172` | error | `{callee}` needs `{name}`, which the call leaves out |
| `S0172` | error | `{owner}` needs field `{}`, which the initialiser leaves out |
| `S0174` | error | a module may have only one `__setup__` block |
| `S0175` | error | `@enable_if` takes one condition |
| `S0175` | error | `{name}` is disabled by `@enable_if` |
| `S0175` | error | `{}` is disabled for these arguments by `@enable_if` |
| `S0175` | error | `{}` is disabled for this member by `@enable_if` |
| `S0175` | error | the condition of `@enable_if` must be a constant |
| `S0176` | error | the specialisation of `{name}<{}>` is ambiguous: {count} match |
| `S0177` | error | `{}` does not satisfy `{base}`: it has no method `{}` with this signature |
| `S0178` | error | `{target_base}` cannot be made from `{source_base}`: it declares no method `{}` |
| `S0178` | error | `{target_base}` cannot be made from `{source_base}`: method `{}` has a different signature |
| `S0178` | error | this call cannot raise, and the restriction marks it with `??` |
| `S0179` | error | （实现内部文本） |
| `S0180` | error | `{name}` omits `{parameter}`, whose default belongs to a template's parameter |
| `S0181` | error | `main` returns `int` or nothing |
| `S0181` | error | `main` takes no parameters, or `argc int, argv **c8` |
| `S0182` | error | a value of type `{}` cannot be written down: declare `__{}__` for it |
| `S0189` | error | `@uninitialized` is for a local binding, not a module-level variable |
| `S0189` | error | `export` applies to a module-level declaration, not a local |
| `S0190` | error | a receiver must be a reference: `this &T` or `this &const T` |
| `S0191` | warning | the result of this `@must_use` call is discarded |
| `S0192` | error | an `except` that names exceptions does not catch the others |

## A.4 警告组

警告编号与命令行的分组开关的对应关系见 [10 诊断 §5](10-diagnostics.zh.md#警告组)。
一个警告组可以包含多个编号；反过来，一个编号只属于一个组（`useless-raise-mark`
的 `S0083` 是唯一同时可能出现在错误与警告上下文中的编号，它在错误上下文里表示
调用点未处理异常，在警告上下文里表示多余的 `??`）。
