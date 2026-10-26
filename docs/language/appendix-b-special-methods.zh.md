# 附录 B 特殊方法表

> [English](appendix-b-special-methods.md)

## B.1 范围

语言在固定构造上会调用的全部特殊方法，以表列出。规范性正文见
[07 表达式 §8](07-expressions.zh.md#特殊方法)；本附录是参考，不创造任何规则。

## B.2 如何读这些表

各表列的含义一致：

| 列 | 含义 |
| -- | ---- |
| **形式** | 方法，即类型可以声明的那些名字 |
| **含义** | 语言要求该类型完成什么 |
| **原型** | 推荐声明：接收者、形参与返回类型 |
| **调用点** | 触发它的构造 |
| **备注** | 何时被选中、没有它时如何、它不得做什么 |

### 约定

B.2.1 `this` **必须**是特殊方法的第一个形参，且**必须**是引用：`this &T` 表示可写，
`this &const T` 表示只读。指针接收者不符合本规范
（[04 函数 §5](04-functions.zh.md#成员函数)）。

B.2.2 原型是**推荐**写法。调用规则真正读取的是接收者的 `const`、形参个数与返回类型；
凡返回类型有讲究之处，都在备注中说明。

B.2.3 `__init__` 只用于显式构造；`__im_init__` 还用于隐式初始化。`__as__` 只用于显式
转换；`__im_as__` 还用于隐式转换。声明 `im` 形式即同时声明了普通形式
（[07 表达式 §8.6](07-expressions.zh.md#隐式初始化与隐式转换)）。

B.2.4 类型要在其他模块中拥有某个特殊方法，该方法**必须**与类型一并 `export`
（[08 模块](08-modules.zh.md#导出表)）。

B.2.5 可能抛出的方法声明为 `except`，语言隐式调用它时同样适用
[06 异常](06-exceptions.zh.md)中的调用点义务。

## B.3 构造、赋值与析构

| 形式 | 含义 | 原型 | 调用点 | 备注 |
| ---- | ---- | ---- | ------ | ---- |
| `__init__(this &T)` | 默认构造 | `fn __init__(this &T) void` | `T x;` | 缺省提供：内建成员置零，声明类型成员调用其自身的默认构造，`@uninitialized` 下不初始化（B.4.1） |
| `__init__(this &T, other &const T)` | 拷贝构造 | `fn __init__(this &T, other &const T) void` | `T b = a;`、按值传参 `f(a)` | 缺省提供：逐成员拷贝 |
| `__init__(this &T, other &&T)` | 移动构造 | `fn __init__(this &T, other &&T) void` | `T b = move(a);` | 缺省提供：逐成员拷贝，且**不清空**源（B.4.2） |
| `__init__(this &T, 形参…)` | 显式构造 | `fn __init__(this &T, n int) void` | `T(a, b)`、`T x(a, b);` | 不提供缺省；必须声明 |
| `__im_init__(this &T, 形参…)` | 隐式初始化 | `fn __im_init__(this &T, text *const c8) void` | `T x = 值;`、实参、返回值、字段 | 同时声明 `__init__`；`im` 形式才是让转换隐式发生的原因 |
| `__assign__(this &T, other &const T)` | 拷贝赋值 | `fn __assign__(this &T, other &const T) &T` | `a = b;` | 缺省提供：逐成员拷贝 |
| `__assign__(this &T, other &&T)` | 移动赋值 | `fn __assign__(this &T, other &&T) &T` | `a = move(b);` | 缺省提供：逐成员拷贝，源**不清空**（B.4.2） |
| `__drop__(this &T)` | 析构 | `fn __drop__(this &T) void` | 作用域结束、被覆盖、`erase`/`pop` | 缺省提供：执行成员各自的析构。声明它用来释放类型自己拥有的资源 |
| `__getitem__(this &T, i usize)` | 下标 | `fn __getitem__(this &T, i usize) &T` | `t[i]` | 返回引用时 `t[i] = v` 可写；类型正是靠这一点获得可写下标 |

## B.4 缺省形式

B.4.1 不声明任何构造函数的类型仍然具有缺省的 `__init__(this &T)`：逐个初始化每个
成员——内建成员置零，声明类型成员调用**它自己**的默认构造，因此任意层嵌套都成立。以
`@uninitialized` 声明的值完全不初始化。`T{…}` 与 `.{…}` **不是**构造调用：字段被直接
写出，任何 `__init__` 都不会运行。

B.4.2 缺省的拷贝与移动形式逐成员拷贝。移动形式**不清空源**——`int` 成员保持原值，
指针成员仍指向原来的目标。清空被移动过的值是源类型自己的责任，只有声明了移动形式才能
做到。读取被移动过的值是**有定义的**（就是移动留下的样子），实现**应**给出警告
`S0148`（`use-after-move`）。

B.4.3 声明这五种形式之一只替换该签名的缺省，其余四种仍如上所述。若成员无法默认构造、
拷贝、移动或赋值，则使用到对应缺省形式时程序非法；从未使用该形式的类型不报错。

B.4.4 右值引用 `&&T` 由标准库的 `move` 产生。它声明在 `std.util`，伞模块 `std` 并不
转出它：使用它的文件**必须**写 `from std.util import move;`。

## B.5 转换与文本

| 形式 | 含义 | 原型 | 调用点 | 备注 |
| ---- | ---- | ---- | ------ | ---- |
| `__as__(this &const T, sel *U)` | 显式转换 | `fn __as__(this &const T, sel *double) double` | `x as U` | 第二个形参是**选择器**：它指明目标类型，且从不被读取 |
| `__im_as__(this &const T, sel *U)` | 隐式转换 | `fn __im_as__(this &const T, sel *double) double` | 实参、初始化器、返回值 | 同时声明 `__as__`；这才是让转换隐式发生的原因 |
| `__str__(this &const T)` | 给读者看的文本 | `fn __str__(this &const T) String` | `str(x)` | 缺省为 `repr(x)` |
| `__repr__(this &const T)` | 给程序写的文本 | `fn __repr__(this &const T) String` | `repr(x)` | 缺省为 `<Type object at 0x…>` |
| `__format__(this &const T)` | 标准库格式化 | `fn __format__(this &const T) String` | `std.format("{}", x)` | 缺省为 `str(x)`；`{!s}`、`{!r}` 在调用点选择 |
| `__hash__(this &const T)` | 散列 | `fn __hash__(this &const T) usize` | 散列容器 | 不提供缺省：用作键的类型必须声明 |

B.5.1 转换不能靠返回类型来区分，因此目标类型由选择器指明：`*double` 选中 `double`。
比较时忽略 `const`。

## B.6 运算符与迭代

| 形式 | 含义 | 原型 | 调用点 | 备注 |
| ---- | ---- | ---- | ------ | ---- |
| `__add__` `__sub__` `__mul__` `__div__` `__mod__` `__pow__` | 算术 | `fn __add__(this &const T, other &const T) T` | `+ - * / % **` | `/` 与 `%` 向零截断 |
| `__and__` `__or__` `__xor__` `__shl__` `__shr__` | 位运算 | `fn __and__(this &const T, other &const T) T` | `& \| ^ << >>` | |
| `__neg__` `__pos__` `__not__` | 一元 | `fn __neg__(this &const T) T` | `-a`、`+a`、`!a` | `__not__` 是逻辑非；按位取反是另一种形式 |
| `__eq__` `__ne__` `__lt__` `__le__` `__gt__` `__ge__` | 比较 | `fn __eq__(this &const T, other &const T) bool` | `== != < <= > >=` | 结果类型来自方法；用作条件时取 `bool` |
| `__cmp__(this &const T, other &const T)` | 三路比较 | `fn __cmp__(this &const T, other &const T) int` | `<=>` | 负、零、正 |
| `__iadd__` … `__ishr__` | 复合赋值 | `fn __iadd__(this &T, other &const T) &T` | `a += b` | 没有它时 `a = a + b`。表达式的值就是存入的值 |
| `__radd__` … `__rshr__` | 交换操作数 | `fn __radd__(this &const T, other &const T) T` | `a` 的类型没有 `__add__` 时的 `a + b` | 仅当左操作数没有该方法时才尝试 |
| `__inc__` `__dec__` | 自增、自减 | `fn __inc__(this &T) &T` | `++a`、`a++`、`--a`、`a--` | 前缀给出新值，后缀给出旧值；操作数**必须**是位置 |
| `__deref__(this &const T)` | 解引用 | `fn __deref__(this &const T) &U` | `*a` | 必须返回引用 |
| `__pointer__(this &const T)` | 经指针访问成员 | `fn __pointer__(this &const T) &U` | `a->field` | 返回下一个要访问的对象；可链式 |
| `__call__(this &T, 形参…)` | 当作函数调用 | `fn __call__(this &T, n int) int` | `a(实参)` | |
| `__bool__(this &const T)` | 真值 | `fn __bool__(this &const T) bool` | `if (a)`、`!a`、`a && b` | **应**产生 `bool` |
| `__iter__` `__next__` | 迭代 | 见 [std.iter](../standard/) | `for (x T : a)` | |

## B.7 运算符速查

前面的表以方法为索引，回答“这个名字做什么”。这一张以**写出来的符号**为索引，回答
“编译器会去找哪个方法”。

| 写法 | 方法 | 写法 | 方法 |
| ---- | ---- | ---- | ---- |
| `a + b` | `__add__` / `__radd__` | `a += b` | `__iadd__`，否则 `__add__` |
| `a - b` | `__sub__` / `__rsub__` | `a -= b` | `__isub__`，否则 `__sub__` |
| `a * b` | `__mul__` / `__rmul__` | `a *= b` | `__imul__`，否则 `__mul__` |
| `a / b` | `__div__` / `__rdiv__` | `a /= b` | `__idiv__`，否则 `__div__` |
| `a % b` | `__mod__` / `__rmod__` | `a %= b` | `__imod__`，否则 `__mod__` |
| `a ** b` | `__pow__` / `__rpow__` | `a **= b` | `__ipow__`，否则 `__pow__` |
| `a & b` | `__and__` / `__rand__` | `a &= b` | `__iand__`，否则 `__and__` |
| `a \| b` | `__or__` / `__ror__` | `a \|= b` | `__ior__`，否则 `__or__` |
| `a ^ b` | `__xor__` / `__rxor__` | `a ^= b` | `__ixor__`，否则 `__xor__` |
| `a << b` | `__shl__` / `__rshl__` | `a <<= b` | `__ishl__`，否则 `__shl__` |
| `a >> b` | `__shr__` / `__rshr__` | `a >>= b` | `__ishr__`，否则 `__shr__` |
| `-a` | `__neg__` | `+a` | `__pos__` |
| `!a` | `__not__` | `~a` | 按位取反 |
| `++a`、`a++` | `__inc__` | `--a`、`a--` | `__dec__` |
| `a == b` | `__eq__` | `a != b` | `__ne__` |
| `a < b` | `__lt__` | `a <= b` | `__le__` |
| `a > b` | `__gt__` | `a >= b` | `__ge__` |
| `a <=> b` | `__cmp__` | | |
| `*a` | `__deref__` | `a->field` | `__pointer__` |
| `a[i]` | `__getitem__` | `a(实参)` | `__call__` |
| `str(a)` | `__str__`，否则 `__repr__` | `repr(a)` | `__repr__` |
| `a as T` | `__as__` / `__im_as__` | `if (a)` | `__bool__` |
| `T x;` | `__init__(this &T)` | `a = b` | `__assign__` |
| `T(实参)` | `__init__(this &T, 形参…)` | 作用域结束 | `__drop__` |

B.7.1 只有在左操作数类型没有普通形式时，才会尝试 `r` 形式：`a + b` 中 `a int`、
`b T` 会到达 `T.__radd__(b, a)`，它的 `this` 是右操作数 `b`。

B.7.2 复合赋值先尝试 `i` 形式。没有它就退化为 `a = a + b`，那需要 `__add__` 与
`__assign__`。

## B.8 约束


B.8.1 特殊方法**必须**是成员函数：第一个形参是 `this`。

B.8.2 `this` **必须**是引用——要写入用 `&T`，不写入用 `&const T`。

B.8.3 `__as__` 与 `__im_as__` 的第二个形参**必须**是选择器。

B.8.4 `__inc__`、`__dec__` 以及各赋值形式的操作数**必须**是位置。

B.8.5 两个相同的声明类型不会被自动比较、也不逐成员比较：需要比较的类型**必须**声明
它（[07 表达式 §8.4](07-expressions.zh.md#运算符与比较)）。

B.8.6 特殊方法与普通成员函数一样参与重载决议，因此一个类型可以为不同来源声明多个
`__init__`、`__assign__` 或 `__as__`。

B.8.7 `i` 形式与 `r` 形式在以上各表中不单列条目；它们是基础方法在复合赋值规则与交换
操作数规则下的产物。
