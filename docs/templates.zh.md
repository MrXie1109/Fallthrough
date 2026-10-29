# 模板参数包：折叠、计数、取元素

> [English](templates.md)

`...` 在模板里有三种用法，同一个名字 `values` 三处都能读：

```ft
@constexpr
template <values ...int>
fn sum() int {
    return (values + ...);          // 折叠：把元素依次合并
}

@constexpr
template <values ...int>
fn count() usize {
    return sizeof...(values);       // 计数：包里有几个元素
}

@constexpr
template <values ...int>
fn second() int {
    return values[1];               // 取元素：下标必须是常量
}

import C;

fn main() int {
    C.printf("%d %zu %d\n", sum<1, 2, 3>(), count<1, 2, 3>(), second<10, 20, 30>());
    return 0;
}
```

类型包同理，`template <Args ...type>` 的 `Args` 可以展开成别的模板的参数
（`Box<Args...>`），也可以 `sizeof...(Args)` 数个数。

## 折叠

折叠有**四种形式**，与 C++ 相同。两种一元形式只取元素，两种二元形式另外给一个起点；
每一对的区别是**从哪一头开始合并**：

以 `values` 是 `<1, 2, 3>`、`init` 是 `100`、`op` 是 `-` 为例：

| 写法 | 结合顺序 | 展开 | 结果 |
| ---- | -------- | ---- | ---- |
| `(values - ...)` | 从右往左：`e1 op (e2 op (… op eN))` | `1 - (2 - 3)` | `2` |
| `(... - values)` | 从左往右：`((e1 op e2) op …) op eN` | `(1 - 2) - 3` | `-4` |
| `(values - ... - 100)` | 从右往左，最后与 `init` 合并 | `1 - (2 - (3 - 100))` | `-98` |
| `(100 - ... - values)` | 从左往右，从 `init` 起 | `((100 - 1) - 2) - 3` | `94` |

- **任何二元运算符都行**，运算符决定“怎么合并”，形式决定“从哪头开始”。
- 二元形式的**起点是一个表达式**，必须是常量；它读作一个**一元表达式**，所以
  `(a + b + ... x)` 要写成 `((a + b) + ... x)`——`+` 是折叠自己的运算符，不是起点里的。
- **空包**：一元折叠需要一个单位元，语言只给了两个——`&&` 是 `true`、`||` 是 `false`；
  其它运算符（`+`、`-`、`*`……）没有单位元，`sum<>()` 报
  “folding it has no value”。二元折叠的空包就是它的起点：`(v + ... + 0)` 配空包是 0。
- 元素必须是**值**：`template <Ts ...type>` 上写 `(Ts + ...)` 会报 “its
  elements are types”，因为类型里没有数。

括号是折叠的一部分，不能省：`values + ...` 里 `...` 不是操作数，只有括号说明
“从这里到那里是一个折叠”。

## 计数 `sizeof...(name)`

返回 `usize`，是**元素个数**，不是字节数——包没有内存布局，能问的只有长度。

`sizeof...` 和 `sizeof(T)` 是两件事，靠 `...` 区分：前者数一个包，后者量一个
类型。空包计数是 **0**，不报错：数个数不需要单位元。

因为结果是编译期常量，它能直接当数组长度：

```ft
@constexpr
template <values ...int>
fn buffer() int {
    a [sizeof...(values)]int;       // 三个元素就是 3 个 i32
    a[2] = 5;
    return a[2];
}

import C;

fn main() int {
    C.printf("%d\n", buffer<1, 2, 3>());   // 5
    return 0;
}
```

## 取元素 `name[index]`

下标必须是**常量整型**，因为元素只存在于编译期，运行期没有任何东西能命名它们：

```ft
values[1]                         // 常量，可以
values[sizeof...(values) - 1]     // 常量表达式，可以：最后一个元素
values[i]                         // i 是运行期参数 → 报错
```

越界在编译期报出来，并给出包里实际有几个元素。取出来的还是元素原本的类型
（`template <values ...int>` 上取出来就是 `int`）。

## 值参数在函数体里是什么

`template <values ...int>` 的 `values` **不是变量**，没有存储、没有地址；它在
实例化时被一组数替换。所以：

* 它只能用在编译期能回答的地方：折叠、计数、取常量下标、以及常量表达式；
* `values[0]` 出现在哪里，那里就得到那个数的字面值。

（`template <N int>` 这样的单个值参数同理。）

## 常量位置上可以用

`constexpr` 模板的调用就是常量，所以它能直接站在要求常量的地方：

```ft
@constexpr
template <values ...int>
fn sum() int {
    return (values + ...);
}

fn main() int {
    a [sum<1, 2, 3>()]int;          // 六个 int
    x int = 0;
    if (sum<1, 2>() == 3) @constexpr {
        x = 7;                      // 只有这一支被生成
    }
    return x;
}
```

要求常量的位置（数组长度、`if constexpr` 的条件）会**先检查里面的表达式**，
再折叠——这正是让模板调用来得及被实例化的那一步。

前提是模板体里要算的东西是**表达式级**的：折叠、`sizeof...`、字面量与它们的运算由
编译器直接化简，所以 `return (values + ...);` 可以站在常量位置；带语句的模板函数体
（先绑定一个局部量、再用循环累加）还不是常量表达式，编译器会说它不是它能求值的函数——
把它写成一次折叠，或者当普通函数调用。

## 函数形参包：另一件东西

上面讲的都是**模板参数包**：`template <values ...int>` 是一串编译期的值，参数表里
一个名字代表一串。函数还能有**形参包**，它是一串**运行期的实参**：

```ft
template <Ts ...type>
fn total(values ...Ts) int {
    sum int = 0;
    (sum += values, ...);
    return sizeof...(values) as int + sum;
}

import C;

fn main() int {
    C.printf("%d\n", total(1, 2, 3));   // 9：三个实参，加上 1 + 2 + 3
    return 0;
}
```

两者写法像，作用却相反：模板参数包由调用点**显式或由模板推导**给出，一次调用只有
一份；形参包由**实参个数**决定，每个个数都展开成一个具体函数（符号里带上元素类型
和个数）。所以 `sizeof...(values)` 在这里数的是“这次调用传了几个”，不是“模板里列了
几个”。形参包的完整规则——`...&Ts` 的引用元素、`f(values...)` 的实参位展开、逗号
折叠必须写成语句——在 [`04-functions.zh.md`](language/04-functions.zh.md#形参包) 里。

## 目前的限制

* **带语句的模板函数体还不是常量表达式**（见上）。当普通调用它是可以的，只是不能
  站进要求常量的位置。
* **`sizeof...` 的对象必须是包**。在既没有模板参数包也没有形参包的地方写
  `sizeof...(x)`，会说 `x is not a template parameter pack`。

## 跨模块：特殊方法也要 `export`

一个模块导出一个类型时，这个类型的 `__init__` / `__assign__` / `__drop__` 也
得一起 `export`，别的模块才调得到——这不是析构的特例，是所有名字统一的可见性
规则。`std.String` 就是这么写的：

```ft
export fn __drop__(this &String) void { ... }
```

只导出 `type Thing` 而不导出它的 `__drop__`，使用方会认为这个类型没有析构，
于是不调用它（也不报错）。std 的两个容器都导出了，所以在任何 `import std;` 的
程序里析构都会正常跑。

## 和 `Array`/`String` 的关系

`std.Array<T>` / `std.String` 是普通模板类型，写法见
[`docs/collections.zh.md`](collections.zh.md)。`Array<Array<int>>` 这种嵌套不用加
空格：类型参数表里的 `>>` 会被当作两个 `>` 拆开；两个**值**之间的 `>>` 仍然
是右移。
