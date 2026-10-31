# 03 声明

> [English](03-declarations.md)

## 范围

本篇规定模块层与块层的声明形式：变量与常量、初始化、声明修饰符、属性、宏、
`traits`、`interface`、模板与类型族，以及要求编译期常量的位置。函数声明见
[04 函数](04-functions.zh.md)。

## 声明前缀

2.1 一个声明的前缀**必须**按下列顺序书写；其他顺序不符合本规范：

```text
@属性 …                    声明属性，各自一行（见第 9 节）
template <…>               模板头
fn / type / union / enum / interface / traits / NAME …
```

2.2 存储类、对齐与常量性不是前缀：它们是属性，写在函数声明之前或变量、
常量、字段的声明之后（见第 9 节）。`mutable` 是字段修饰符，写在字段类型之后。

2.3 声明与作用域中名字的可见顺序无关（见第 8 节前置声明与
[09 作用域](10-scopes.zh.md)）。

## 绑定

3.1 一个绑定由名字、类型与可选的初始化器构成。

3.2 没有初始化器的绑定按位零初始化。因此任何类型都存在合法的初始状态。

3.3 `const` 限定的绑定**必须**有初始化器。

3.4 `auto` 从初始化器推断类型：

```ft
import C;

fn main() int {
    v int = 5;
    p *int = &v;
    n auto = 7;
    r auto = *p;        // 值为类型：auto 推断的是值
    C.printf("%d %d\n", n, r);
    return 0;
}
```

该程序输出 `7 5`：`n` 是 `int`，`r` 是指针所指的那个 `int`，而不是指针本身。

3.4.1 `auto` 推断**值**的类型：初始化器为引用时，结果为被引用值的类型。

3.4.2 初始化器是取上下文类型的字面量时，`auto` 取该上下文类型。

3.4.3 `type(auto)` 推断**表达式本身**的类型，与 `type(expr)` 一致；地方仍为引用。

3.4.4 `auto` 只能用于有初始化器的绑定与函数返回类型。形参类型**必须**写出。

3.4.5 修饰符中的 `auto` 按间接层匹配：`x *auto = e;` 中的 `*` 消耗 `e` 上的一层
指针或引用，`auto` 取剩余类型；`x &auto = e;` 要求 `e` 是地方。

## 声明修饰符

4.1 声明层面的存储类、对齐与常量性由属性表示，导出由关键字 `export` 表示；
位置与含义见第 9 节：

```ft
import C;

count int @static = 0;          // 静态存储期
seen int @thread_local = 0;     // 每个线程一个对象
buf [16]byte @align(64);        // 抬高对齐
export fn visible() void { }    // 导出函数

fn main() int {
    count = 3;
    buf[0] = 1;
    C.printf("%d %d %d\n", count, seen, buf[0] as int);
    return 0;
}
```

该程序输出 `3 0 1`：`count` 保留赋给它的值，`seen` 从零开始，`buf` 的首字节可写。

4.2 类型限定词 `*`、`&`、`&&`、`const`、`volatile` 写在类型之前；`mutable` 写在
字段类型之后，表示该字段可以在 `const` 成员函数中写入。

4.3 `@align` 的约束见 [02 类型](02-types.zh.md#布局)。

## 模块级变量

5.1 模块层绑定声明一个程序范围内唯一的对象，初始化一次，生命延续到程序结束。

5.2 模块级变量的初始化器**必须**是编译期常量。没有初始化器时值为零。

5.3 模块级变量可以被赋值。

5.4 未标记 `export` 的模块级变量只在其定义模块内可见；`export` 的规则见
[08 模块](09-modules.zh.md#导出表)。

### 外部变量与外部函数

5.5 模块级变量标上 `@extern_C`，就声明了一个由外部库提供的对象；函数声明同理。语言里
没有 `extern` 关键字：属性说明这个名字来自别处，写在类型之后——与其他声明属性同一个位置。

```ft
import C;

@extern_C
fn getenv(name *const c8) *c8;

fn main() int {
    path *c8 = getenv("PATH");
    C.printf("%d\n", (path != nullptr) as int);
    return 0;
}
```

在设置了 `PATH` 的环境里该程序输出 `1`：`getenv` 按它自己的名字在 C 库中查找、使用 C 调用
约定，因为 `@extern_C` 是这么说的。

5.6 外部变量**不得**有初始化器；读写该名字访问库提供的存储。

5.7 `@extern_C` 同时指定调用约定与符号来源：名字不加修饰地交给链接器，且不生成定义。
没有函数体的函数声明，正是同一个道理。

5.8 `export` 使外部变量或外部函数对导入者可见。

## 模块级常量

6.1 模块级常量是带 `const` 类型的模块层绑定：`LIMIT const usize = 1024;`。

6.2 模块级常量**必须**用编译期常量初始化：它的值随程序镜像一起确定，并且可以用
在要求编译期常量的位置（见第 14 节）。

6.3 局部 `const` 绑定只表示声明后不可写，初始化器可以不是常量：值在运行到该声明
时求出。

6.4 绑定上的 `@constexpr` 是更强的承诺：它**必须**有初始化器，且初始化器**必须**
是编译期常量；该绑定同时是 `const`（见第 9 节）。

6.5 常量与变量都可以导出：`export` 写在声明之前
（`export LIMIT const usize = 8;`、`export counter int = 0;`）。

## `__setup__`

7.1 一个模块**至多**有一个 `__setup__` 函数，声明为 `fn __setup__() void`。

```ft
import C;

counter int = 0;

fn __setup__() void {
    counter = 41;
}

fn main() int {
    C.printf("%d\n", counter + 1);
    return 0;
}
```

该程序输出 `42`：`main` 读到的是 setup 函数运行之后的 `counter`，所以 `__setup__`
里的赋值就是 `main` 看到的值。

7.2 它在 `main` 之前运行一次，顺序按导入依赖：先运行被导入模块的 `__setup__`，
后运行导入者的。

7.3 它是一个函数，所以函数体与任何 `void` 函数一样写：声明绑定、调用函数、可以
用 `return;` 提前结束。它**不得**返回值或 `raise`，也不接受参数。

7.4 `__setup__` 是普通标识符。只有当声明恰为 `fn __setup__()` 时它才是模块的
setup 函数；其他签名只是没人调用的普通函数。

## 前置声明

8.1 名义类型可以只声明、后定义：

```ft
import C;

type Node;
union Value;
enum Color u32;
traits Walkable(it int);

type Node {
    value int;
    next *Node;
}

fn main() int {
    n Node = .{7, nullptr};
    C.printf("%d %d\n", n.value, (n.next == nullptr) as int);
    return 0;
}
```

该程序输出 `7 1`：`Node` 在被写出字段之前先被命名，而到构造值时该类型已经完整。

8.2 定义出现之前，该类型是不完整类型：只能用于指针、引用与声明；按值定义、
`sizeof`、`alignof`、成员访问与按值传参**必须**要求完整类型。

8.3 声明与定义是同一名字的两次出现；两次定义不符合本规范。

8.4 `enum` 的前置声明**必须**写出基类型（省略时为 `int`）；成员随定义出现。

8.5 只有定义跨模块导出。导入方看到的类型**必须**是完整的。

8.6 `interface` 的前置声明与 `traits` 相同。

## 属性

### 形式与位置

9.1 属性写作 `@name` 或 `@name(args)`。一个声明可以带多个属性；属性之间顺序无关。

9.2 属性按被描述对象分四类，位置各不相同：

| 类别 | 位置 |
| ---- | ---- |
| 函数 | 声明之前，包括 `template <…>` 之前 |
| 变量、常量、字段 | 名字与类型之后；有初始化器时在 `=` 之前 |
| 分支 | 条件之后、块之前：`if (cond) @likely {`、`else @unlikely {`、`case 1 @likely:` |
| 类型、类型族、traits、interface、enum、别名 | 声明之前 |

9.3 把属性写在不允许的位置、名字未知、同一属性重复、参数个数或类别不符都是错误；
实现**不得**静默忽略。

### 函数属性

| 属性 | 含义 |
| ---- | ---- |
| `@inline` | 请求内联 |
| `@noinline` | 请求不内联 |
| `@noreturn` | 函数不会正常返回；返回类型**必须**是 `void` |
| `@must_use` | 调用结果**必须**被使用 |
| `@cold` | 函数很少被调用，代码生成可以放慢路径 |
| `@deprecated`、`@deprecated("说明")` | 使用时产生警告 |
| `@enable_if(cond)` | 条件为假时声明不存在；条件**必须**是编译期常量 |
| `@constexpr` | 函数可以在常量位置调用（第 14 节） |
| `@extern_C` | 使用 C ABI，符号不改编 |

### 变量与常量属性

| 属性 | 含义 |
| ---- | ---- |
| `@inline` | 允许在多个翻译单元中定义同一个变量 |
| `@deprecated`、`@deprecated("说明")` | 使用时产生警告 |
| `@align(N)` | 对齐要求，规则见 [02 类型](02-types.zh.md#布局) |
| `@constexpr` | 必须有编译期常量初始化器；绑定同时是 `const` |
| `@static` | 静态存储期（局部） |
| `@thread_local` | 每线程一个对象 |
| `@uninitialized` | 不零初始化、不调用空构造；**不得**有初始化器；只允许局部绑定 |
| `@maybe_unused` | 不报告“从未读取”警告 |

### 类型、分支与模块

9.4 类型、类型族、traits、interface、enum 与类型别名可以由 `export` 导出，
也可以带 `@deprecated`；类型声明还可以带 `@must_use`，表示返回该类型的函数的调用结果
**必须**被使用。

9.5 分支属性：

| 属性 | 位置 | 含义 |
| ---- | ---- | ---- |
| `@likely` | `if`、`else`、`case` | 该分支通常被选中 |
| `@unlikely` | `if`、`else`、`case` | 该分支很少被选中 |
| `@constexpr` | 仅 `if` 的条件后 | 条件**必须**是编译期常量；未选中的分支不检查 |

9.6 模块与宏的导出也用关键字：`export from std.array import *;` 重新导出伞模块的
名字；`export define NAME …` 使宏对导入者可见。

9.7 `export` 可以标记任何可导出的实体：函数、变量、常量、类型、类型族、
traits、interface、enum、伞模块 re-export 与宏定义。

## 宏

10.1 宏是记号替换，定义在模块层：

```ft
import C;

define MAX(a, b) ((a) > (b) ? (a) : (b))
define SHOW(x) #x
export define VERSION 3

fn main() int {
    C.printf("%d\n", $MAX(3, 7));
    C.printf("%s\n", $SHOW(hello));
    C.printf("%d\n", $VERSION);
    return 0;
}
```

该程序依次输出 `7`、`hello`、`3`：`MAX` 就是它被定义成的那个条件表达式，`#x` 把实参
变成文本 `hello`，`VERSION` 就是它被定义成的数。每处使用都带着 10.4 要求的 `$`。

10.2 `define NAME body` 的定义延伸到逻辑行尾；行尾的 `\` 使定义延续到下一行。

10.3 `export define` 使宏对导入者可见。

10.4 宏调用**必须**带 `$`：`$NAME` 或 `$NAME(args)`。不带 `$` 的名字不是宏调用。

10.5 展开是递归的，并受实现规定的深度限制。超过限制时实现**必须**报告诊断。

10.6 `#param` 把实参转换为字符串字面量；`a ## b` 把相邻两个记号拼接为一个。

10.7 `...` 声明可变参数，变参以 `__VA_LIST__` 访问。

10.8 `define` 是保留字，但宏定义由预处理阶段识别：只有逻辑行起始处的
`define` / `export define` 引导定义，函数体内的 `define` 不是宏定义。

10.9 宏名可以模块限定：`import meta;` 后 `$meta.FILE` 展开为 `meta` 导出的
`FILE`。点号仅当整串名字是宏名时属于名字。

10.10 实现提供内建宏：`$__builtin_FILE`、`$__builtin_LINE`、`$__builtin_COLUMN`、
`$__builtin_PLATFORM`、`$__builtin_ARCH`、`$__builtin_OS_WINDOWS`、
`$__builtin_OS_POSIX`、`$__builtin_DATE`、`$__builtin_TIME`。它们展开为字面量，
不依赖模块。

## traits

### 定义

11.1.1 `traits Name(params) { requirements }` 声明一个具名要求集合。

11.1.2 参数是值参数（名字在前、类型在后）。参数类型中出现的名字是该 trait 的
**类型变量**；值参数在要求中是地方。

11.1.3 要求是以逗号分隔的表达式。每条要求可以带结果约束：

- `-> !void`：结果存在且不是 `void`；
- `-> T`：结果类型恰为 `T`；
- `-> 值`：结果的编译期值恰为该值。

11.1.4 要求中的方法调用可以带类型实参（`it.push(int)`）。实参类型**必须**是
类型。

11.1.5 要求中调用后写 `??` 表示该操作**可能抛出**。

11.1.6 trait 之间没有继承子句；层级通过“前一条要求写成要求”表示，例如
`InputIterator(type(it)) -> true`。

11.1.7 `export` 使 trait 可被导入者查询。

### 查询

11.2.1 把 trait 当作函数写，得到编译期 `bool`：`InputIterator(IntIter)`。

11.2.2 查询的实参**必须**是类型。查询一个值的类型写作 `type(expr)`。

11.2.3 trait 查询折叠为常量，但其类型是普通 `bool`。因此它可以出现在常量位置，
也可以存入 `bool` 变量。

11.2.4 查询未声明的 trait 或实参个数不符是错误。

### 检查时机

11.3.1 要求只在 trait 被查询时检查，声明处不检查。

11.3.2 未被查询的 trait 不产生运行期或代码生成代价；其中写错的要求不在声明处
报告。

### 内建类型特征

11.4.0 类型特征回答的是**类型自身**的问题——它是不是某个类别、两个类型是否等价、
移动它是否可能抛出——而不是"它有哪些成员、能进行哪些操作"。后者由程序用
`traits` 声明（如标准库的 `InputIterator`、`Allocator`）。

11.4.1 类别谓词由标准库用**类型家族与模板特化**定义，不是编译器内建。
`std.type_traits` 为每个谓词声明一个类型家族：泛型成员回答 `false`，对满足谓词的
类型写一条显式特化回答 `true`。

```ft
import C;

template <T type>
type IsInteger {
    VALUE const bool = false;
}

template <>
type IsInteger<int> {
    VALUE const bool = true;
}

fn main() int {
    C.printf("%d %d\n", IsInteger(int) as int, IsInteger(f64) as int);
    return 0;
}
```

该程序输出 `1 0`：`IsInteger(int)` 选中特化，其 `VALUE` 为 `true`；`IsInteger(f64)`
回落到通用版本，其 `VALUE` 为 `false`。

11.4.2 查询 `IsInteger(int)` 实例化家族；特化机制（完整特化或偏特化，最具体者胜）
选择回答的成员，常量成员 `VALUE` 折叠为答案，因此查询仍是编译期常量。程序可以
按同样的形状声明自己的谓词：家族体内声明 `VALUE const bool` 常量，该名字就是
一个查询。

11.4.3 下列查询由标准库 `std.type_traits` 定义，随 `import std` 或
`import std.type_traits` 可用：

| 查询 | 含义 |
| ---- | ---- |
| `IsInteger(T)` | 是整数类型 |
| `IsFloating(T)` | 是浮点类型 |
| `IsNumeric(T)` | 是整数或浮点 |
| `IsPointer(T)`、`IsReference(T)` | 是指针 / 引用 |
| `IsVoid(T)`、`IsBool(T)`、`IsCharacter(T)` | 依次对应 |

11.4.4 两个谓词用偏特化表达，也在 `std.type_traits` 中：

| 查询 | 含义 | 写法 |
| ---- | ---- | ---- |
| `IsArray(T)` | 是定长数组类型 | 偏特化 `IsArray<[]T>` 匹配任意长度的数组 |
| `IsSame(A, B)` | 两个类型等价 | 偏特化 `IsSame<A, A>` 把两个位置绑定到同一类型 |

11.4.5 移动一个 `T` 是否可能抛出，问的是 `T` 被选中的移动构造上的 `except` 标记：
`std.util.move_can_raise<T>()` 返回 `bool`。它不能直接用作
`if (..) @constexpr` 的条件，因为条件里的函数调用不可折叠，可折叠的是函数体中的
`except(..)` 查询；因此 `std.util` 提供一个包装宏：

```ft
import C;
import std;
import std.util;

template <T type>
fn move_path() int {
    if ($MOVES(T)) @constexpr {
        C.printf("move may raise, so the element is copied\n");
    } else {
        C.printf("move cannot raise, so the element is moved\n");
    }
    return 0;
}

fn main() int except {
    move_path<int>();
    move_path<String>();
    return 0;
}
```

该程序两次输出 `move cannot raise, so the element is moved`：`int` 与 `String` 的移动都
不会抛异常，所以两者都走移动路径。

`$MOVES(T)` 展开为查询本身 `except(T(util.fake<&&T>()))`，所以条件在书写处即可
折叠。使用它需要 `import std.util;`。

11.4.6 所有类型特征的结果都是编译期常量，可用于 `if (..) @constexpr` 与
`@enable_if`。标准库其余的类型要求（迭代器、分配器、可格式化等）都是库内声明的
`traits`，不是类型特征。

## interface

### 定义与语义

12.1.1 `interface Name(it T, …) { requirements }` 声明一个运行期装箱类型。

12.1.2 声明写法与 `traits` 相同；区别在用途：`traits` 的名字只用于查询，
`interface` 的名字是一个类型。

12.1.3 把具体类型的值赋给接口类型时，实现**必须**检查该类型的值满足接口的每条
要求；满足时在堆上创建副本并生成方法表。

12.1.4 经接口调用的方法**必须**按其方法表项进行虚调用。

12.1.5 接口值可转换为方法更少、签名相同的接口。

### 方法表

12.2.1 一条要求如果写得出结果类型，则是方法表的一项。

12.2.2 方法名是要求中调用的成员名；参数类型是要求中显式写出的类型；结果类型是
`-> T`；`??` 表示该方法可能抛出。

12.2.3 写不出结果类型的要求只是检查项，不是方法表项；经接口调用它不是本规范
允许的构造。

12.2.4 方法表顺序为要求书写顺序。

## 模板

### 参数

13.1.1 模板由 `template <params>` 引导。

| 写法 | 含义 |
| ---- | ---- |
| `T type` | 类型参数 |
| `N int` | 值参数；实参**必须**是常量 |
| `Args ...type`、`values ...int` | 参数包；必须是最后一个参数，接受零个或多个实参 |

13.1.2 每个不同的实参组合产生独立的实例化。值实参不同即为不同实例。

13.1.3 模板实参**必须**是常量，可以显式写出或由调用处推断。

13.1.4 模板体按定义它的模块解释名字。导入方声明同名类型导致实例化歧义时，实现
**必须**报告诊断，不得静默合并。

### 默认实参

13.2.1 参数可以带默认值：类型参数写 `= 类型`，值参数写 `= 表达式`。

13.2.2 带默认值的参数**必须**排在无默认值参数之后；参数包不受此限。

13.2.3 默认值可以使用其前面的参数。

### 特化

13.3.1 类型族的成员可以全特化（`template <>`）或部分特化（模式中含参数）。

13.3.2 匹配时最特殊者胜出。两个模式互不包含时是歧义，实现**必须**报告诊断。

13.3.3 特化只替换字段；类型族的方法按成员的实参代入。

13.3.4 特化随导出的类型族跨模块可见。

### 类型族的方法

13.4.1 类型族的方法是一个模板函数，接收者写明族名与参数（`this &Vec<T>`）。

13.4.2 方法的 `template` 头**不得**省略。

13.4.3 为具体成员声明的方法与族方法同名时，具体成员方法优先。

13.4.4 族与方法各自 `export` 才能被导入者使用。

## 编译期常量位置

14.1 下列位置的值**必须**在编译期已知：

- 数组长度 `[N]T` 中的 `N`；
- 模板实参；
- `if (cond) @constexpr` 的条件；
- `const` 绑定的初始化器；
- `enum` 成员的值；
- `case` 标签；
- `static_assert` 的条件。

14.2 站在这些位置的调用**必须**调用带 `@constexpr` 的函数，且实参**必须**是常量。

14.3 `@constexpr` 是声明上的承诺；站在常量位置的那次调用**必须**能够求值，求值
过程中读取的每个值都**必须**是常量，否则实现**必须**报告诊断。

14.4 模板函数的常量求值是表达式级的：折叠、包计数、字面量与它们的运算可以直接
求值；含语句的模板函数体不是常量表达式。

14.5 未标记 `@constexpr` 的函数可以被实现折叠，但那是优化而非语言承诺；程序不得
依赖其发生或不发生。
