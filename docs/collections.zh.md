# `std.Array` 与 `std.String`

> [English](collections.md)

标准库的两个容器。它们拥有自己的内存，所以本文大部分篇幅在讲**谁负责哪块内存**。

- [拿到它们](#拿到它们)
- [`Array<T>`](#arrayt)
- [`String`](#string)
- [遍历](#遍历)
- [移动：构造与赋值](#移动构造与赋值)
- [引用与指针](#引用与指针)
- [多文件](#多文件)
- [限制](#限制)

## 拿到它们

```ft
import std;              // 伞模块：std.Array<T>、std.String、std.Box<T>
import std.array;        // 只要 Array
import std.string;       // 只要 String（Exception 也在这里）
import std.memory;       // 内存、panic、Box、以及 fake/move 探针
import std.algorithm;    // 算法：sort、reverse、find、fill…
```

下面的例子用 `C.printf` 打印，所以它们还要 `import C;`：`std` 不会转发 C 的绑定，
两套库是分开的。

伞模块导出的**类型名**可以直接写短：

```ft
import std;
fn main() int {
    xs Array<int>;       // 与 std.Array<int> 是同一个类型
    s String;
    return 0;
}
```

**函数不能**写短：`import std;` 之后要写 `std.out.print(s)`，不能写 `print(s)`。理由
见 [08 模块](language/08-modules.zh.md#短名类型名字和值名字不一样)。

### 其余入口

语言参考里引用到、但不属于 `Array`/`String` 的那些名字，都在 `std` 里：

| 名字 | 签名 | 用途 |
| ---- | ---- | ---- |
| `std.panic` | `(*const c8) void` | 消息打到标准错误，然后 abort |
| `std.alloc` | `(usize) *byte` | `malloc`；可能返回 `nullptr` |
| `std.allocate` | `(usize) *byte` | 同上，但失败时 `panic` |
| `std.calloc` | `(usize, usize) *byte` | 清零分配 |
| `std.realloc` | `(*byte, usize) *byte` | 重分配 |
| `std.free` | `(*byte) void` | 释放 |
| `std.move` | `<T> (&T) &&T` | 把一个有名字的值当作将死的值交给构造函数：`b String = std.move(a);` |
| `std.fake` | `<T> () T` | 只出现在编译期查询里（`except(std.fake<&T>().__drop__())`），运行期调用会 abort |
| `util.__ft_moves` | `<T> () bool` | 编译期问题：移动 `T` 会不会抛；`Array` 用它决定搬还是拷 |
| `std.Box<T>` | `__init__(&Box<T>)`、`__init__(&Box<T>, T)`、`get()`、`empty()`、`clone()` | 堆上一个值，析构时释放；**移动**语义，拷贝用 `clone()` |
| `std.Exception` / `std.exception()` | `name()`、`what()` 各返回 `String` | 正在处理的异常，见 [06 异常](language/06-exceptions.zh.md) |

**模块名会撞车**：导入的名字按模块名在搜索路径里找文件，所以你自己写一个叫
`math` 的模块会和 `standard/std/math.ftm` 撞名，症状通常是
``module `math` does not export `square` ``。解决办法是改自己的模块名，或用
`import geom.math;` 这样带目录的名字。

## `Array<T>`

`Array<T>` 是一个类型族，每个元素类型一个成员：`Array<int>` 放 `int`，
`Array<Point>` 放 `Point`。索引是**元素下标**，不是字节偏移。

```ft
import std;
fn main() int {
    xs Array<int>;
    xs.push(3);
    xs.push(4);
    xs.push(5);
    return xs[0] + xs[2];        // 8
}
```

### 构造

| 写法 | 含义 |
| ---- | ---- |
| `a Array<int>;` | 空数组，不分配（容量 0） |
| `b Array<int> = Array<int>(3, 7);` | 三个 7 |
| `c Array<int> = a;` | **拷贝**构造：元素逐个拷贝，buffer 是新的 |
| `d Array<int> = make();` | **移动**构造：接管 `make()` 的 buffer |

`Array<T>` 的布局是「buffer 指针 + 长度 + 容量」，所以

```ft
sizeof(Array<int>)      // 24
sizeof(*int)            // 8，一个指针
```

### 元素、长度、容量

| 成员 | 说明 |
| ---- | ---- |
| `length()` / `capacity()` | 元素个数 / 已分配容量 |
| `empty()` | `length() == 0` |
| `data()` | `*const T`，借出 buffer；数组被修改或销毁后失效 |
| `a[i]` | 元素引用：`&T`（const 数组上是 `&const T`） |
| `front()` / `back()` | 首/尾元素引用，空数组是错的 |
| `push(v)` | 末尾追加 |
| `pop()` | 弹出末尾元素并**返回**（按值） |
| `insert(pos, v)` / `erase(pos)` | 中间插入 / 删除，O(n)；`pos` 是迭代器（`begin() + i`），不是下标 |
| `reserve(n)` / `shrink_to_fit()` | 预留 / 收到刚好够用 |
| `resize(n, filler)` / `clear()` | 改长度 / 清空 |

### 算法

排序、查找、反转、填充都在 [`std/algorithm.ftm`](../../standard/std/algorithm.ftm)，
是**自由函数**，不是 `Array` 的成员——C++ 的写法：容器管存储，算法管一段
**范围**（`begin()` 到 `end()` 的那对位置）。

每个算法只有**一个模板**，按迭代器类型工作，而不是每种容器一个重载：算法体只使用
位置上的语言操作（`++`、`*`、`==`/`!=`，需要随机访问时再加 `+`），所以 `Array` 的
`*T` 位置、`String` 的字节位置和 `Deque` 的迭代器都调用同一个 `std.sort`。函数的
`@enable_if` 约束把这件事写给编译器：`sort`/`reverse` 要求
`RandomAccessIterator`，`is_sorted`/`fill`/`swap` 要求 `ForwardIterator`，
`find`/`index_of`/`count_of` 要求 `InputIterator`。

```ft
import std;
fn main() int {
    values Array<int>;
    values.push(3);
    values.push(1);
    values.push(2);
    descending Greater<int>;
    std.sort(values.begin(), values.end());            // 1 2 3
    std.sort_by(values.begin(), values.end(), descending); // 3 2 1
    return 0;
}
```

| 自由函数 | 说明 |
| -------- | ---- |
| `sort(first, last)` | 按 `<` 排序，堆排序，最坏 `O(n log n)`，不额外分配 |
| `sort_by(first, last, order)` | 按给定的比较器排序；`order` 是一个值，比如 `Greater<int>` |
| `is_sorted(first, last)` / `is_sorted_by(first, last, order)` | 是否已经有序 |
| `reverse(first, last)` | 就地反转 |
| `fill(first, last, value)` | 每个槽位都写入 `value` |
| `index_of(first, last, value)` / `count_of(first, last, value)` | 第一个相等元素的下标（没有时 `-1`） / 相等元素个数 |
| `find(first, last, value)` | 指向第一个相等元素的位置，没有时返回 `last` |
| `swap(first, second)` | 交换两个位置上的元素，走类型自己的拷贝、赋值和移动 |

位置是**迭代器**的容器（`Deque`）用迭代器对调用同一组算法：
`std.sort(d.begin(), d.end())`、`std.is_sorted`、`std.reverse`、`std.fill`、
`std.index_of`、`std.count_of` 都是这个拼写。

交换元素走元素类型自己的拷贝构造、赋值和移动，所以 `Array<String>` 排完之后每个
元素仍然各有一块 buffer。交换元素走元素类型自己的拷贝构造、赋值和移动，所以 `Array<String>` 排完之后每个
元素仍然各有一块 buffer。

### 元素是构造出来的，不是拷进去的

槽位在放进第一个值之前是**裸内存**。往里建一个值调用元素类型的
`__init__(value)`；对已经活着的元素赋值调 `__assign__`；销毁调 `__drop__`。
`Array` 的内部实现就是这么写的：

```ft
slot *T = this._data + this._len;
slot->__init__(value);          // 在裸槽位里构造
```

基本类型不需要声明这三者——编译器给 `int` 之类的类型内建的版本：
`__init__`/`__assign__` 就是一次 store，`__drop__` 什么都不做。所以同一段代码
对 `Array<int>` 和 `Array<String>` 都成立。

结果是 **`Array<String>` 和 `Array<Array<int>>` 是深拷贝**：元素各有自己的
buffer，改源不影响元素。

```ft
import std;
fn main() int {
    a String = "hello";
    xs Array<String>;
    xs.push(a);                  // 元素里的 buffer 是另一块
    a.push('!');                 // 改源
    return xs[0][0] as int;      // 104，'h'：元素仍是 "hello"
}
```

嵌套模板参数**不用加空格**：`Array<Array<int>>` 里的 `>>` 在类型参数表里
会被当作两个 `>` 拆开；两个值之间的 `>>` 仍然是右移。

## `String`

`String` 是三个字（`sizeof(String)` = 24），末尾总有一个 `'\0'`，所以 `data()`
可以直接交给 C。

**扩容时移动元素**：`Array` 在扩容（`reserve`/`shrink_to_fit`）和 `insert` 时，如果元素类型的移动构造不会抛，就把元素搬过去而不是拷贝——`String` 的 buffer 因此原地易主，地址不变。判断由编译器在实例化时回答（`util.__ft_moves<T>()`），所以每个元素类型各自选择；会抛的移动构造退回拷贝，免得异常发生时旧 buffer 里一半元素已被搬空。`if constexpr` 决定哪条分支存在。

**分配失败不算异常**：`push`、`insert`、`reserve`、`resize`、`shrink_to_fit`、
`+=`、`+`、`*` 都不标 `except`。内存不足时它们走 `std.panic`（带信息 abort）——
分配失败既没有值可以交回来，也没有地方可以恢复，把它做成调用点义务只会让每个
`push` 都套一层 `try`。异常系统只留给程序自己 `raise` 的错误。

**短字符串不分配内存（SSO）**：不超过 22 字节的文本直接存在 `String` 对象自己的
三个字里——最后一个字节的高位标记"内联"，其余位存长度。这样的字符串创建、拷贝、
销毁都不碰分配器；`capacity()` 报的是可容纳的字节数（含终止符），内联字符串是 23
——24 字节的对象里，两个字节给了标记与终止符。

```ft
import std;
fn main() int {
    short String = "hi";                 // 内联，没有 malloc
    long String = "this text is long enough to allocate for sure";
    C.printf("%lu %lu\n", short.capacity(), long.capacity());   // 23 46
    short.shrink_to_fit();               // 已经内联，什么也不做
    long.clear();
    long.push('a');
    long.shrink_to_fit();                // 短到能进对象，于是搬回对象里
    return 0;
}
```

| 成员 | 说明 |
| ---- | ---- |
| `length()` / `capacity()` / `empty()` | `capacity()` 在两种形态下含义一致：可容纳的字节数（含终止符） |
| `data()` | `*const char`，C 字符串；永远不会是 null（空串也有自己的终止符） |
| `s[i]` | `&char`，可读可写 |
| `push(ch)` / `push(&const String)` / `push(*const char)` | 追加字符 / 字符串 / C 字符串 |
| `insert(i, ch)` / `insert(i, &const String)` | 中间插入 |
| `erase(i)` | 删除第 `i` 个字符并**返回**它 |
| `reserve` / `shrink_to_fit` / `resize` / `clear` | 同 `Array` |

内建运算符：

```ft
import std;
fn main() int {
    a String = "ab";
    b String = "cd";
    c String = a + b;            // __add__，"abcd"
    a += '!';                     // __iadd__，"ab!"
    d String = b * 2;             // __mul__，"cdcd"
    say String = str(c);          // __str__，给人和 `std.out` 看的
    return c.length() as int;     // 4
}
```

比较（`==`、`<` 等）接 `&const String`，所以两边都可以是临时量或名字。`repr`
给调试看，带引号；`str` 不带。

### 查找与切分

文本本身的查询是成员函数，参数一律接 `&const String`，所以名字和临时量都能传：

```ft
import std;
fn main() int {
    text String = "  hello world  ";
    where isize = text.find(String("world"));      // 8
    missing isize = text.find(String("zzz"));      // -1
    has bool = text.contains(String("hello"));     // true
    starts bool = text.starts_with(String("  he"));
    ends bool = text.ends_with(String("  "));
    trimmed String = text.trimmed();               // "hello world"
    piece String = text.substring(2, 5);           // "hello"
    return 0;
}
```

`find` 返回 `isize`，因为「没有」必须是一个位置取不到的值；只想要是/否就用
`contains`。`substring(start, count)` 把范围夹到文本内：越过末尾要多少给多少，
起点越界给空串——读文件时「给我最后一行」本来就可能比剩下的多。

`split(separator char, into &Array<String>)` 把每一段**追加**到数组里。相邻分隔符
之间、开头之前和结尾之后都算一段，只是那段是空的；所以 `"a,b,"` 是三段，空串是
一段：

```ft
parts Array<String>;
String("a,b,,c").split(',', parts);    // ["a", "b", "", "c"]
```


### 转换

文本读成数字走 `as`：`String` 提供 `__as__` 转换，目标是 `int`、`usize`、`f64`。
**整段文本都必须是这个数**：`"12x"`、`""`、`"1 2"` 会抛 `ValueError`；整数与
`usize` 的转换拒绝超出范围的值，而不是悄悄回绕。转换可能失败，所以调用处按普通异常
规则处理（`??` 传出去，或 `try`/`except` 接住）：

```ft
n int = String("-42") as int??;            // -42
size usize = String("100") as usize??;     // 100
real f64 = String("2.5e2") as f64??;       // 250.0
```

`std.in.input(n)` 也走这条转换，输入失败同样抛 `ValueError`。判断「这是不是数」而不是
要求一个数，用 `try`/`except` 接住 `ValueError`，或者先检查文本再转换。

## `BasicString<Char>` 与 `String16`/`String32`

`String` 是库自己用的 `char` 字符串（小串内联在对象里，见上一节）。`BasicString<Char>`
是同一套字符串形状的**通用形式**：一段 `Char`、长度、以及读取/追加/比较等操作
（[`std/basic_string.ftm`](../../standard/std/basic_string.ftm)）：

```ft
text BasicString<char> = BasicString<char>("hello");
text.push('!');                 // hello!
joined BasicString<char> = text + BasicString<char>(" world");
```

它只有一块缓冲区（没有小串优化），构造、`push`、`length`、`[]`、`begin`/`end`、
`+`、比较、`find`/`contains` 都在；`char` 形式还能从 C 字符串字面量构造，并可以用
`__str__` 变成 `String`。

更宽的字符类型用别名：`String16 = BasicString<u16>`、`String32 = BasicString<u32>`
（在 `std` 伞模块里导出）。它们没有字面量语法，从 `Array<u16>`/`Array<u32>` 构造：

```ft
units Array<u16> = Array<u16>();
units.push(72 as u16);
wide String16 = String16(units);
wide.push(33 as u16);
```

## `Pair`/`Triple` 与 `Variant2`

`Pair<A, B>` 与 `Triple<A, B, C>` 是固定的小组合：字段 `first`/`second`（以及
`third`）是公开的，直接读写位置。构造、拷贝、移动、析构与 `==` 都按元素做，所以
`Pair<String, int>` 拷贝时复制文本而不是共享同一个缓冲区
（[`std/tuple.ftm`](../../standard/std/tuple.ftm)）：

```ft
point Pair<int, int> = Pair<int, int>(3, 4);
text Pair<String, int> = Pair<String, int>(String("hi"), 7);
```

`Variant2<A, B>` 是"二者之一"：`is_first()`/`is_second()` 说明当前是哪个，
`first()`/`second()` 取出对应的值，取错时抛 `TypeError`。两个可选值各用一个指针
存储、只在被选中时分配，所以未选中的一边是空指针而不是未初始化的字节
（[`std/variant.ftm`](../../standard/std/variant.ftm)）：

```ft
value Variant2<int, String> = Variant2<int, String>(5);
if (value.is_first()) {
    std.print("{}\n", value.first()??)??;
}
value.set_second(String("text"));
```

## `Any`：任意类型的值

`Any` 用一个持有者装不同类型的值：值按**文本**存，同时记住类型名，取回时按类型名
检查，问错抛 `TypeError`（[`std/any.ftm`](../../standard/std/any.ftm)）：

```ft
a Any = Any(42);
std.print("{} {}\n", a.type_name(), a.to_int()??)??;   // int 42
b Any = Any(String("hi"));
std.print("{}\n", b.to_text()??)??;                    // hi
```

支持 `bool`/`int`/`f64`/`String`（以及字面量与 C 字符串）；`present()`/`if (a)`、
`type_name()`、`text()`、`reset()`、`==` 也在。它适合配置值、JSON 节点这类"值是
运行时才知道的类型"的场合；代价是相等按渲染文本比较，且不保留类型的更多信息。

## `Bitset`、`Ring`、`List`

三个固定形状的容器：

- **`Bitset`**（[`std/bitset.ftm`](../../standard/std/bitset.ftm)）：固定数量的位，按
  `u64` 字打包。`set(i)` 返回该位原来的值，`test`/`clear_bit`/`toggle`、`count_ones`/
  `count_zeros`/`any`/`none`、`merge`/`intersect`、`clear`/`set_all`。越界与 `Array`
  的 `[]` 一样会 panic。

  ```ft
  bits Bitset = Bitset(70usize);
  bits.set(3);
  n usize = bits.count_ones();      // 1
  ```

- **`Ring<T>`**（[`std/ring.ftm`](../../standard/std/ring.ftm)）：固定容量的先进先出
  缓冲。`push` 返回是否放得下（满时**不覆盖**最旧值）；`pop()` 返回 `?T`；还有
  `front`/`back`/`get`/`length`/`capacity`/`empty`/`full`/`clear`。槽位是原始内存，
  元素在 push 时构造、pop 时析构。

- **`List<T>`**（[`std/list.ftm`](../../standard/std/list.ftm)）：双向链表，节点逐个
  分配，位置稳定，两端插入 O(1)，但不能按下标访问。`push_back`/`push_front`、
  `pop_front`/`pop_back` 返回 `?T`、`front`/`back`、`length`/`empty`/`clear`；可用
  `for (value &const T : list)` 遍历（`ListIterator<T>` 提供 `begin`/`end`）。

## `Bytes`：字节序列

`String` 的“无类型”兄弟：同样是一段连续字节，但不承诺这些字节是文本。
文件、哈希、协议数据、甚至一份 `String` 的拷贝，都是字节序列，`Bytes` 就是
说“字节”而不是“文本”的那个类型（[`std/bytes.ftm`](../../standard/std/bytes.ftm)）：

```ft
data Bytes = "hello";         // 复制字面量的字节
data.push(33 as byte);
n usize = data.length();      // 6
first u8 = data[0];           // 'h'
```

- 存储与增长就是 `Array<u8>` 的：`length`/`empty`/`push`/`reserve`/`resize`/`clear`/
  `begin`/`end` 都转发给它；`data()` 给出 `*byte`，`const_data()` 给出 `*const byte`。
- 与文本互转走 `as`：`b as String` 逐字节复制成文本（不解码），`s as Bytes` 反向复制。
- 比较是逐字节的：`==`/`!=` 只在长度相同且每个字节都相同时为真。

## 遍历

`Array<T>` 和 `String` 都能被 `for (x T : ...)` 直接遍历：它们提供
`begin()`/`end()`，返回的迭代器提供 `__inc__`、`__deref__`、`__ne__`。这就是
语言的那五个操作，没有别的东西——`std/iterator.ftm` 只是给它们起了名字
(`Iterable`、`InputIterator`)，编译器不要求你导入任何 trait 才能写循环。

```ft
for (x int : xs) {          // 一份拷贝
    C.printf("%d\n", x);
}
for (x &int : xs) {         // 元素本身：写回数组
    x = x * 2;
}
for (c char : s) {          // String 的字节
    C.printf("%c", c);
}
```

迭代器**就是指针**：`Array<T>` 的 `begin()`/`end()` 返回 `*T`，`String` 的返回
`*char`。元素在内存里连续，所以"位置"本身就是一个地址，`++`、`*`、`!=`、`[]` 和比较
都是语言的指针运算，没有包装类型要写。`std/iterator.ftm` 把它们归入
`Iterable → InputIterator → ForwardIterator → BidirectionalIterator →
RandomAccessIterator` 这条链的最上层。

这些类型各有名字，名字在容器里：

| 名字 | 是什么 |
| ---- | ------ |
| `Array<T>.Iterator` | `*T`：正向、可写 |
| `Array<T>.ConstIterator` | `*const T`：正向、只读 |
| `Array<T>.ReverseIterator` | 包一个 `*T` 的逆向位置，`++` 往回走 |
| `Array<T>.ConstReverseIterator` | 同上，只读 |

逆向迭代器遵循 C++ 的约定：它**指在**它产出的元素之后，所以 `rbegin()` 就是正向走完
时的位置，`*it` 是前一个元素，`++it` 向开头走。它只有十几个字节和一次指针减法。

```ft
it Array<int>.ReverseIterator = a.rbegin();
for (; it != a.rend(); ++it) {
    C.printf("%d ", *it);
}
```

### 用位置修改

`insert` 与 `erase` 取的是**位置**（也就是迭代器），不是下标：

```ft
a.insert(a.begin() + 1, 9);   // 在第二个元素前插入
gone int = a.erase(a.begin()); // 删掉第一个，返回它
```

`String` 两种都收：`s.insert(3, 'x')` 用字节下标，`s.insert(s.begin() + 3, 'x')` 用
位置；`erase` 同理。程序通常手上有一个下标，而循环与迭代器给的是位置。

两句话由此而来：

- 遍历中**不要增长**容器——增长会换掉缓冲区，指针就指到旧的那块了；
- 遍历中**可以**改元素：缓冲区和元素位置都没变。

`begin()`/`end()` 只接受可写的容器（`this &Array<T>`、`this &String`）：遍历一个
`&const Array<T>` 需要一套 const 迭代器，现在还没有。内联存储的短字符串（字节就
在对象里）遍历的是对象本身，所以遍历期间也不能让它变长。

## 移动：构造与赋值

一个值什么时候被“移动”而不是“拷贝”，只看**它是不是将死的**：调用结果、
构造出来的临时量是将死的；有名字的变量不是。

```ft
import std;

fn make() String {
    s String = "hello, this is long enough to need a heap buffer";
    return s;                     // s 移出函数
}

fn main() int {
    a String = "named";
    b String = a;                 // 拷贝：a 还活着
    c String = make();            // 移动：接管 make 的 buffer
    d String; d = make();         // 移动赋值：先释放 d 的旧 buffer
    a = b;                        // 拷贝赋值
    return 0;
}
```

规则是：**将死的实参优先选 `&&` 形式，否则选 `&const` 形式**。这条对构造函数
和 `__assign__` 一视同仁——`d = make()` 走移动赋值，`a = b` 走拷贝赋值。所以
`Array`/`String` 里 `__assign__` 的拷贝形式声明在移动形式**之前**，与这条规则
配合。

移动之后源是**空**的：buffer 为 null、长度 0、容量 0。这个状态仍然是一个合法
值：`data()` 返回 `""`，可以继续 `push`，也可以正常销毁（释放 null 是安全的）。

## 引用与指针

这两件事在语言里是分开的，容器的接口照此写：

- `&x` 得到的是**指针** `*T`，永远不是引用；
- 引用（`&T`）只在类型的写法里出现，是“这个名字就是那块存储”，不加 `&`；
- `__getitem__` 返回 `&T`，所以 `a[i] = v` 直接可用，没有 `__setitem__`：

```ft
import std;
fn main() int {
    xs Array<int>;
    xs.push(1);
    xs[0] = 7;                    // 元素是 &int，可以直接赋值
    return xs[0];                 // 7
}
```

同样的原因，`front()`/`back()` 返回的引用可以直接赋值，而 `pop()`/`erase()` 是
**按值返回**——它们把元素从数组里拿出来，数组不再拥有它。

方法也可以直接调用在一个表达式的结果上，编译器为它安排一块临时存储：

```ft
import std;
fn main() int {
    xs Array<String>;
    a String = "hello";
    xs.push(a);
    return xs.pop().length() as int;    // 5，只弹一次
}
```

元素是结构体时，`a[i].field` 直接可用——`a[i]` 是那个元素本身，不是它的副本：

```ft
import std;
type Point { x int; y int; }
fn main() int {
    ps Array<Point>;
    first Point = .{1, 2};
    ps.push(first);
    ps[0].x = 10;                 // 写元素自己的字段
    return ps[0].x + ps[0].y;     // 12
}
```

## 多文件

一个模块导出类型时，它的特殊方法也要一起导出，别的模块才调得到：

```ft
// thing.ftm
export type Thing { v int; }
export fn __drop__(this &Thing) void { /* ... */ }
```

```ft
// main.ft
import C;
import thing;
fn main() int {
    t Thing = thing.make_thing(9);
    C.printf("%d\n", t.v);
    return 0;
}
```

不导出的名字是本模块的私事——包括 `__init__`/`__assign__`/`__drop__`。少了导出
不会报错，使用方只是“认为这个类型没有析构”。

## 限制

这些是现在做不到的事，而不是待修的 bug：

- **`.{ ... }` 不能直接作为 `push` 的实参**（`xs.push(.{1, 2})` 会说“需要上下文
  给出类型”）。先绑一个名字再 push；`a[i].field = v` 和 `a[i] = v` 都可以。
- **`insert`/`erase` 收的是位置（迭代器）**，不是下标：`a.insert(a.begin() + 1, v)`。
  给数组传下标会被当作地址，结果是运行期失败而不是编译错误。`String` 是例外：
  `s.insert(1, 'X')` 与 `s.erase(1)` 都收下标。
- `data()` 借出的指针在数组被修改后失效，语言不会检查这一点。
- 遍历要求**可写**的容器：`begin()` 没有 `&const` 版本，因此 `for` 不能遍历一个
  `&const Array<T>`。

更细的模板与编译期内容（参数包、折叠、`sizeof...`）见
[`docs/templates.zh.md`](templates.zh.md)；还没定的设计问题见
[`docs/SPEC-STATUS.md`](SPEC-STATUS.md)。
