# 05 语句

> [English](05-statements.md)

## 范围

本篇规定语句的形式与语义：块、条件、循环（含范围形式）、`switch`、跳转、延迟块与
编译期断言。异常语句见 [06 异常](06-exceptions.zh.md)。

## 语句形式

2.1 语句包括：绑定、赋值、复合赋值、表达式语句、裸块、`if`、循环、`switch`、
跳转语句、`defer` 与 `static_assert`。

2.2 任何具有值的表达式都可以作为表达式语句；其值被丢弃。

2.3 裸块 `{ … }` 引入一个作用域；作用域结束时其中的绑定被销毁。

2.4 条件与循环的头**必须**写括号：`if (...)`、`while (...)`、`for (...)`、
`switch (...)`。

2.5 控制流块之后**不得**写分号；声明与表达式语句以 `;` 结束。

## 条件

### 真值

3.1.1 `if`、`while`、`for` 的条件接受下列类型：

| 操作数类型 | 真值 |
| ---------- | ---- |
| 整数、浮点、字符 | 值非零为真 |
| `bool` | 其值 |
| 指针 | 非空为真 |
| 引用 | 被引用值为真时真 |
| 数组 | 恒真 |
| 函数 | 恒真 |
| 其他名义类型 | 要求声明 `__bool__` |

3.1.2 条件读取的是操作数的真值，而不是把操作数转换为 `bool`。该调整属于
[02 类型](02-types.zh.md) 第 12 节列出的隐式调整。

3.1.3 `!x` 的结果类型是 `bool`，接受与条件相同的操作数。

### `if (cond) @constexpr`

3.2.1 `if (c) @constexpr` 的条件**必须**是常量。

3.2.2 条件为假时选择 `else` 分支。

3.2.3 未被选中的分支**不检查**，且不生成代码。

3.2.4 普通 `if` 检查两个分支；条件恰为常量时只生成选中的分支，另一分支仍被
检查。

### 分支权重

3.2.5 `if`、`else` 与 `case` 之后可以写分支权重属性：`@likely` 表示该分支通常被
选中，`@unlikely` 表示很少被选中。

```ft
import C;

fn run() int { return 1; }
fn wait() int { return 2; }

fn classify(code int) int {
    switch (code) {
        case 200 @likely:
            return 1;
        default:
            return 0;
    }
}

fn main() int {
    ready bool = true;
    if (ready) @likely {
        C.printf("run=%d\n", run());
    } else @unlikely {
        C.printf("wait=%d\n", wait());
    }
    C.printf("%d %d\n", classify(200), classify(500));
    return 0;
}
```

该程序先输出 `run=1`，再输出 `1 0`。权重是对代码生成的建议，不是条件：走哪个分支
只由 `ready` 与 `code` 决定。

3.2.6 分支权重只影响代码生成，不改变求值顺序与结果；同一分支**最多**写一个权重
属性。把它写在其他位置是错误。

3.2.7 在需要值的位置上，两个分支都有的 `if` 本身就是那个值：每个分支是一个块，块给出什么，
`if` 就给出什么（见 [09 作用域 §2.2](09-scopes.zh.md#作用域)）。两个分支**必须**给出相同的
类型；没有 `else` 的 `if` 给不出值。两边都是单个表达式时，条件表达式 `cond ? a : b` 表达
同一件事。

```ft
import C;
import std;

fn sign(n int) int { return if (n < 0) { -1 } else { 1 }; }

fn main() int except {
    C.printf("%d %d\n", sign(-5), sign(5));
    return 0;
}
```

该程序输出 `-1 1`：这个 `if` 就是 `sign` 返回的值，每个分支的块给出其中写下的那个数。

### 编译期断言
### 编译期断言

3.3.1 `static_assert(cond)` 或 `static_assert(cond, message)` 在编译期检查
`bool` 常量；条件为假时实现**必须**报告诊断，并在有 `message` 时将其写入诊断。

3.3.2 `static_assert` 可以出现在模块层与函数体中；**不得**出现在类型定义体内。

3.3.3 模板中的 `static_assert` 在每个实例化时检查一次；未实例化的模板不触发它。

3.3.4 条件**必须**是编译器能够求值的常量。

### 探测一个块能否编译

3.4.1 `if (auto) @constexpr { … } else { … }` 请编译器**试着**编译后面的块。条件不写：
问题就是那个块能否编译。能，则块就是程序；不能，则 `else` 块是，而试编期间产生的诊断
被丢弃。

3.4.2 没被选中的一侧**完全不作检查**，所以块里可以写对某些类型并不存在的名字。只有致命
诊断才让试编失败：警告——比如某个变量从未被读过——不算。

3.4.3 问一个**名字**能否编译用 `@meta.compilable(T)`（见
[07 表达式 §7.9](07-expressions.zh.md#编译期查询)）；问一个**块**能否编译用
`if (auto) @constexpr`。

```ft
import C;
import std;

type Point { x int; y int; }

fn main() int except {
    if (auto) @constexpr {
        p Point = .{1, 2};
        n int = p.x;
        C.printf("a Point has x\n");
    } else {
        C.printf("no point here\n");
    }
    return 0;
}
```

该程序输出 `a Point has x`：块能编译，所以保留的是它。若块里写了 `Point` 没有的字段，
走的就会是 `else`，而对这次试编不会报告任何东西。
## 循环

4.1 三种循环形式都可以带 `else` 子句。

```ft
import C;

fn main() int {
    n int = 0;
    while (n < 3) {
        n = n + 1;
    } else {
        C.printf("while else ran\n");
    }
    for (i int = 0; i < 2; i = i + 1) {
        C.printf("%d ", i);
    } else {
        C.printf("for else ran\n");
    }
    return 0;
}
```

该程序先输出 `while else ran`，再输出 `0 1 for else ran`：循环的 `else` 在循环未经过
`break` 而正常结束时执行，这里两个循环都是如此。

4.1.1 `else` 子句在循环正常结束时执行：条件变为假，或范围形式走到 `end()`。

4.1.2 以下情况**不**执行 `else`：`break`、`return`、`raise`。`continue` 不构成
中断。

### `while`

条件为真时重复执行循环体。条件规则同第 3 节。

### `for (init; cond; update)`

4.3.1 `init` 可以是绑定、表达式语句或空。

4.3.2 `init` 中绑定的作用域为整个 `for` 语句。

4.3.3 `cond` 省略时为恒真。

4.3.4 `update` 在每次循环体结束（以及执行 `continue`）之后执行，然后重新判断
条件。

## 范围循环

### 展开规则

`for (x T : range)` 要求 `range` 支持下列操作：

| 操作 | 含义 |
| ---- | ---- |
| `r.begin()` | 起始位置 |
| `r.end()` | 结束位置：最后一个元素的下一位置 |
| `begin != end` | 是否未走完 |
| `++it` | 前进一个位置 |
| `*it` | 读取当前位置 |

范围循环**必须**按下式展开，其中 `$` 开头的名字由实现生成、程序不可书写：

```text
auto $it = r.begin();
auto $end = r.end();
for (; $it != $end; ++$it) {
    x T = *$it;
    <循环体>
} else {
    <else 子句>
}
```

5.1.1 展开后的代码遵循普通语句的全部规则。

5.1.2 `range` **只求值一次**。若 `range` 是命名存储的表达式（名字、字段、解引用、
常量下标），实现可以直接在其上调用 `begin`/`end`；否则**必须**先绑定临时量。

5.1.3 循环变量为 `&T` 时是元素本身；为 `T` 或 `auto` 时是副本。

5.1.4 遍历期间**不得**使容器增长或重新分配；此类行为是未定义行为。

5.1.5 定长数组按键的元素遍历。它的 `begin()` 与 `end()` 来自标准库，标准库为每一种
元素类型和长度只声明一次：

```ft
export
template <T type, N usize>
fn begin(this &const [N]const T) *const T {
    return &this[0];
}
```

长度 `N` 由实参自身的长度推出，所以一处声明服务所有数组。字符串字面量就是
`[N]c8`，因此按同样的方式遍历，**可以**直接作为范围循环的范围：

```ft
import C;
import std;

fn main() int except {
    for (ch c8 : "abc") {
        C.printf("%c ", ch);
    }
    C.printf("\n");
    return 0;
}
```

该程序输出 `a b c`。

5.1.6 定长数组的元素连续排列，因此迭代器是指向它的指针，`end()` 就是
`begin() + N`。具名数组就地遍历；字面量先绑定到临时对象，这是 5.1.2 的要求。

### 以范围为被遍历对象

5.2.1 遍历一段整数序列的常规写法是范围字面量。`1 .. n` 走 1 到 `n - 1`，`1 ..= n`
走 1 到 `n`（见 [01 词法 §6.6](01-lexical.zh.md#范围字面量)）。循环变量是 `i64`，
即 `Range` 的元素类型：

```ft
import C;
import std;

fn main() int except {
    total i64 = 0i64;
    for (i i64 : 1 ..= 10) {
        total += i;
    }
    C.printf("%ld\n", total);
    return 0;
}
```

该程序输出 `55`，即 1 到 10 之和：`..=` 含上界，所以遍历到 10。

5.2.2 范围循环的对象可以是任何支持上表所列操作的值，范围字面量是其中之一。`Range`
值也可以存下来反复遍历，同一个范围用两次就是走两遍：它是值，不是游标。

5.2.3 给 `[N]T` 补上这两个操作，定长数组就能遍历。长度是模板的值参数，所以一份声明适用
于所有长度；数组字面量也是普通对象：

```ft
import C;
import std;

template <T type, N usize>
fn begin(this &const [N]T) *const T { return &this[0]; }
template <T type, N usize>
fn end(this &const [N]T) *const T { return &this[0] + N; }

fn main() int except {
    for (ch c8 : "abc") { C.printf("%c ", ch); }
    C.printf("| ");
    for (v int : {10, 20, 30}) { C.printf("%d ", v); }
    C.printf("\n");
    return 0;
}
```

该程序输出 `a b c | 10 20 30 `：字符串字面量是 `[N]const c8`，数组字面量是 `[N]T`，
两者都经上面两个声明遍历。末尾的空格是循环体在最后一个元素之后写出的那个。

## `switch`

6.1 `switch (v) { case C: … default: … }` 按值选择分支。

6.2 标签**必须**是常量。一个标签组可以包含多个逗号分隔的常量。

6.3 同一 `switch` 中两个标签**不得**具有相同的值。

6.4 `default` 可以出现在任意位置，至多一个。

6.5 一个分支结束后**不得**落入下一分支；需要落入时**必须**在该分支末尾写
`fallthrough;`。

6.6 每个分支的语句属于 `switch` 的作用域；需要独立作用域时写裸块。

6.7 `switch` 的值**必须**与标签可以比较。

## 跳转

| 语句 | 语义 |
| ---- | ---- |
| `break;` | 离开最近的循环或 `switch` |
| `continue;` | 进入最近循环的下一轮 |
| `return;` | 从返回 `void` 的函数返回 |
| `return x;` | 返回值；类型**必须**与返回类型兼容 |
| `fallthrough;` | 只出现在 `switch` 分支末尾，进入下一分支 |

7.1 `continue` 的目标是最近的**循环**；`switch` 不作为其目标。

7.2 `break` 的目标是最近的循环或 `switch`。

## `defer`

8.1 `defer { … }` 把块登记到**当前作用域**，在作用域结束时执行。花括号**必须**写。

8.2 同一作用域内的多个 `defer` 按**声明顺序的逆序**执行。

8.3 作用域以任何方式结束时 `defer` 都执行：正常走完、`return`、`break`、`continue`，
以及异常离开该作用域。

8.4 进入的作用域总比外层先结束，所以内层块的 `defer` 先于外层块的 `defer`。

8.5 同一作用域结束时，`defer` 块先于该作用域的局部变量析构执行（都按逆序）。
`defer` 块内可以再声明局部变量或再写 `defer`，它们按该块自己的规则结束。

8.6 `defer` 是语句，不是表达式，不能用作值。
