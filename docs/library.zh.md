# 标准库其余部分

> [English](library.md)

[`collections.zh.md`](collections.zh.md) 讲 `Array<T>`、`String` 与 `std` 的那几个入口。
本文接着讲剩下的模块：内存、容器、迭代器、数学、I/O、同步与文件路径。每个模块单独
成文件，所以只想要其中一块的程序可以只导入那一块。

- [拿到它们](#拿到它们)
- [内存：分配器、`Box`、`Rc`、`Arc`](#内存分配器boxrcarc)
- [迭代器 traits](#迭代器-traits)
- [有序容器：`std.tree`](#有序容器stdtree)
- [哈希容器：`std.hash`](#哈希容器stdhash)
- [双端队列与堆：`std.deque`、`std.heap`](#双端队列与堆stddeque-stdheap)
- [数学：`std.math`](#数学stdmath)
- [I/O：`std.io`](#iostdio)
- [同步：`std.sync`](#同步stdsync)
- [文件路径：`std.fs`](#文件路径stdfs)
- [限制](#限制)

## 拿到它们

```ft
import std;          // 伞模块：上面全部
import std.tree;     // 只要 RBTree 与 TSet/TMap
import std.hash;     // 只要 HashTable 与 HSet/HMap
import std.io;       // 只要设备与格式化 I/O
```

和 `collections.zh.md` 说的一样：伞模块导出的**类型名**可以写短（`RBTree<int, int>`
就是 `std.RBTree<int, int>`），**函数**要写全名（`std.min(..)`）。容器模板的
默认参数（比较器、哈希器、分配器）只在模块内部按短名写，伞模块下也一样：
`TMap<String, int>` 用的是 `std.Less<String>`，不用写出来。

## 内存：分配器、`Box`、`Rc`、`Arc`

### 分配器是类型

分配器住在 [`std/allocators.ftm`](../../standard/std/allocators.ftm)：`Allocator` trait
（`alloc`/`free`）、`DefaultAllocator`（C 堆）与新加的 **`Arena`**——一块内存加一个
游标，`alloc` 只是指针前移，`reset()` 一次性归还全部，块用完时 `panic`。它适合
"整批一起死"的场合（编译器一遍、文档解析）：

```ft
import std.allocators;

arena Arena = Arena(64usize);
block *byte = arena.alloc(8usize);   // 未初始化，构造值仍要 __init__
arena.reset();                        // 全部可再用
```


`std.Allocator` 是一个 trait，要求两个方法：

```ft
traits Allocator(it A) {
    fn alloc(this &A, size usize) *byte;
    fn free(this &A, block *byte) void;
}
```

`std.DefaultAllocator` 就是 `malloc`/`free`。`Deque`、`Heap`、树容器、哈希容器和
`Rc`/`Arc` 把分配器作为**模板参数**接收，默认是它（`Array` 只用默认分配器）：

```ft
type MyAllocator { };              // 两个方法各自实现
xs Deque<int, MyAllocator>;        // 用别的方式分配；Array 只收元素类型
```

分配器是值而不是全局钩子。两个容器用不同的分配器，它们的内存互不相干。所有
`alloc` 都可能返回 `nullptr`；容器在失败时 `panic("out of memory")`，和
`std.allocate` 一样。

### 裸分配

`std` 直接暴露 C 的分配函数，签名见 [`c-bindings.zh.md`](c-bindings.zh.md)：

| 名字 | 签名 | 用途 |
| ---- | ---- | ---- |
| `std.alloc` | `(usize) *byte` | `malloc`，可能返回 `nullptr` |
| `std.allocate` | `(usize) *byte` | 同上，失败 `panic` |
| `std.calloc` | `(usize, usize) *byte` | 清零分配 |
| `std.realloc` | `(*byte, usize) *byte` | 重分配 |
| `std.free` | `(*byte) void` | 释放 |

### `Box<T>`

堆上的一个值，析构时释放。

```ft
b Box<int> = 7;        // 从值构造
b.get() = 9;           // get() 返回 &T
c Box<int> = b;        // 拷贝：两份独立的值
```

`Box<T>()` 是空的 `Box`（`empty()` 为 `true`），`get()` 在空的时候 `panic`。

### `Rc<T>` 与 `RcWeak<T>`

引用计数，单线程共享所有权：

```ft
first Rc<String> = String("text");
second Rc<String> = first;        // 计数 2
weak RcWeak<String> = first.downgrade();
std.out.println(weak.upgrade().get());  // 活着的强引用在，就拿到一份
```

- `strong_count()`、`weak_count()` 是当前计数；
- `clone()` 是显式的强引用拷贝，`downgrade()` 产生弱引用；
- 最后一个强引用消失时值被析构，之后 `upgrade()` 返回空的 `Rc`，`expired()` 为
  `true`；
- 空的 `Rc`（默认构造的、或 `upgrade()` 失败得到的）没有值可读：`get()`/`*` 会
  `panic("Rc is empty")`，而不是解引用空指针；
- 弱引用计数最后归零时，控制块本身被释放。

拷贝、移动、赋值、析构都成对实现（`__init__`/`__assign__`/`__drop__`），所以
`Rc<T>` 可以放进容器，也可以被返回。

### `Arc<T>` 与 `ArcWeak<T>`

和 `Rc` 同一个接口，区别只在计数：`Arc` 的计数是 `Atomic<usize>`，可以跨线程
共享。构造、拷贝、移动、赋值、析构、`clone()`、`get()`、`empty()`、
`strong_count()`、`weak_count()`、`downgrade()` 与 `Rc` 一一对应，`ArcWeak` 也有
`upgrade()`/`expired()`。

注意 `Atomic<T>` 目前是**互斥锁支持的**（见 [同步](#同步stdsync)），所以 `Arc` 的
计数正确但不是无锁的。

## 迭代器 traits

`std.iterator` 只有 traits，没有类型。容器是否满足它们由 `traits` 查询在编译期
回答：

| trait | 要求 |
| ----- | ---- |
| `Iterable` | `begin()`/`end()` |
| `InputIterator` | `__inc__`、`__deref__`、`__eq__`/`__ne__` |
| `ForwardIterator` | 在 `InputIterator` 之上可以多次遍历 |
| `BidirectionalIterator` | 再加 `__dec__` |
| `RandomAccessIterator` | 再加 `+`/`-`/`[]` |

查询写成 `InputIterator(MyIterator)`，结果是编译期 `bool`：

```ft
static_assert(InputIterator(SetIterator<int>));
static_assert(BidirectionalIterator(SetIterator<int>));
```

哪一层成立是每个类型的事实，不是一张分类表：

- `std.tree` 的迭代器有 `--`，是**双向**的；
- `std.hash` 与 `std.deque` 的迭代器只能前进（`++`），是 `InputIterator`；
- `Array<T>` 的 `begin()`/`end()` **就是指针**，随机访问是指针算术；要倒着走用
  `rbegin()`/`rend()`，它们返回的迭代器带 `+`/`-`/`[]`，满足
  `RandomAccessIterator`。

## 有序容器：`std.tree`

红黑树，以及建在它上面的四种容器。

### 比较器

```ft
traits Compare(it C, value E) {
    it(a, b) -> bool,
}
```

`std.Less<E>` 与 `std.Greater<E>` 是两个现成的实现。自定义比较器是一个带
`__call__` 的类型，作为模板参数传入：

```ft
TMap<int, String, Greater<int>> descending;
```

比较器回答的是「`a` 是否排在 `b` 前面」，不是三路比较：`Less` 是 `a < b`，
`Greater` 是 `b < a`。

### `RBTree<K, V, Cmp = Less<K>, A = DefaultAllocator>`

| 方法 | 结果 |
| ---- | ---- |
| `insert(key, value)` | 插入一个键，返回 `*TreeNode<K, V>`；键已存在时替换值 |
| `insert_multi(key, value)` | 同键可以有多份 |
| `find(key)` | 找到返回 `*TreeNode<K, V>`（`const` 表返回 `*const`），否则 `nullptr` |
| `contains(key)`、`count(key)` | 是否在、有几份 |
| `lower_bound(key)`、`upper_bound(key)` | 第一个不小于 / 大于 `key` 的节点 |
| `erase(key)`、`erase_all(key)` | 删一份 / 删全部 |
| `clear()`、`length()`、`empty()` | 清空与大小 |

`TreeNode<K, V>` 有 `key`、`value` 与 `red`、`left`、`right`、`parent`。节点的链接
是 `*TreeNode<K, V>` 而不是 `*byte`，所以顺着树走不需要转换：

```ft
node *TreeNode<int, String> = tree.find(3);
if (node != nullptr) {
    std.out.println(node.value);
}
```

### 建在树上的容器

| 类型 | 内容 | 接口 |
| ---- | ---- | ---- |
| `TMap<K, V>` | 一键一值 | `insert`、`find`、`at`、`[]`、`contains`、`count`、`erase`、`clear`、`length`、`empty`、`begin`/`end` |
| `TMMap<K, V>` | 一键多值 | `insert`、`find`、`count`、`erase`、`erase_all`、`clear`、`length`、`empty`、`begin`/`end` |
| `TSet<K>` | 只存键 | `insert`、`contains`、`count`、`erase`、`clear`、`length`、`empty`、`begin`/`end` |
| `TMSet<K>` | 可重复的键 | 同 `TSet`，`insert` 不去重 |

`at`/`[]` 在键不存在时 `panic`，`find` 则返回 `nullptr`：想要可空的结果用 `find`，
想要「必须在」用 `at`。

迭代器是 `MapIterator`/`ConstMapIterator`/`SetIterator`/`ConstSetIterator`：
`__inc__`/`__dec__` 前后移动，`__deref__` 给值，`key()` 给键。所以顺序是键的
顺序，由比较器决定：

```ft
ages TMap<String, int>;
ages.insert(String("Ada"), 36);
ages.insert(String("Grace"), 85);
it MapIterator<String, int> = ages.begin();
stop MapIterator<String, int> = ages.end();
while (it != stop) {           // 键的顺序
    C.printf("%s %d\n", it.key().data(), *it);
    ++it;
}
```

### 有序，但不保证什么

红黑树保证插入、查找、删除都是 `O(log n)`，迭代是键的顺序。同一个键的多次插入
（`insert_multi`）之间的相对顺序**不保证**，因为再平衡会改变同键节点的位置。

## 哈希容器：`std.hash`

链地址法哈希表，以及建在它上面的四种容器。

### 哈希器

```ft
traits Hashable(it T) {
    fn __hash__(this &const T) usize;
}
```

`std.Hash<T>` 是一个**类型**，它的 `__call__` 去问 `value.__hash__()`。所有整数、
`byte`、`bool`、`c8`/`c16`/`c32` 与 `String` 都已经有 `__hash__`。给自定义类型加
哈希就是加一个 `__hash__`：

```ft
type Point { x int; y int; }

export fn __hash__(this &const Point) usize {
    hasher Hash<int>;
    return hasher(this.x) ^ hasher(this.y);
}

export fn __eq__(this &const Point, other &const Point) bool {
    return this.x == other.x && this.y == other.y;
}
```

**两个方法缺一不可。** 哈希决定去哪个桶里找，`__eq__` 决定桶里的哪一项就是那个键；
`Hashable` trait 把这两件事都要求了。给自定义哈希器就是把一个带 `__call__` 的类型
作为模板参数传进去。

### `HashTable<K, V, H = Hash<K>, A = DefaultAllocator>`

| 方法 | 结果 |
| ---- | ---- |
| `insert(key, value)` | 插入或替换 |
| `insert_multi(key, value)` | 同键可以有多份 |
| `find(key)` | `*V` / `*const V`，不在时 `nullptr` |
| `contains(key)`、`count(key)` | 是否在、有几份 |
| `erase(key)`、`erase_all(key)` | 删一份 / 删全部 |
| `clear()`、`length()`、`empty()` | 清空与大小 |

表按装载因子增长并重新哈希，所以 `find` 是均摊 `O(1)`。

`HashEntry<K, V>` 是桶里的一个节点（`key`、`value`、`hash`、`next`），迭代器靠它
前进。它被导出是因为导入的签名会提到它；普通程序不需要写这个名字。

### 建在哈希表上的容器

| 类型 | 内容 | 接口 |
| ---- | ---- | ---- |
| `HMap<K, V>` | 一键一值 | `insert`、`find`、`at`、`[]`、`contains`、`count`、`erase`、`clear`、`length`、`empty`、`begin`/`end` |
| `HMMap<K, V>` | 一键多值 | `insert`、`find`、`count`、`erase`、`erase_all`、`clear`、`length`、`empty` |
| `HSet<K>` | 只存键 | `insert`、`contains`、`count`、`erase`、`clear`、`length`、`empty` |
| `HMSet<K>` | 可重复的键 | 同 `HSet`，`insert` 不去重 |

`HSet`/`HMap` 的迭代顺序**不保证**：它是桶的顺序，和插入顺序、键的顺序都无关。
要按顺序遍历用 `std.tree`。

## 双端队列与堆：`std.deque`、`std.heap`

### `Deque<T, A = DefaultAllocator>`

环形缓冲区，两端都能进出：

| 方法 | 结果 |
| ---- | ---- |
| `push_back(value)`、`push_front(value)` | 加到一端 |
| `pop_back()`、`pop_front()` | 从一端取出，返回该值 |
| `front()`、`back()` | `&T` / `&const T`，空时 `panic` |
| `[]` | 按下标访问，`0` 是前端 |
| `reserve`、`capacity`、`length`、`empty`、`clear` | 容量与状态 |

`DequeIterator` 只能前进（`InputIterator`）；元素本身随机访问，用 `d[i]`。

### `Queue<T>` 与 `Stack<T>`

`Queue<T>` 是 `Deque<T>` 的薄包装：`push`/`pop`/`front`/`length`/`empty`/
`clear`，先进先出。`Stack<T>` 也一样：`push`/`pop`/`top`/`length`/`empty`/`clear`，
后进先出。

### `Heap<T, Cmp = Less<T>, A = DefaultAllocator>`

二叉堆。比较器说「谁该先出去」：`Less` 让**最小**的在前（小顶堆），`Greater` 让
最大的在前。

| 方法 | 结果 |
| ---- | ---- |
| `push(value)` | 插入，`O(log n)` |
| `pop()` | 取出堆顶，`O(log n)` |
| `top()` | `&T` / `&const T`，空时 `panic` |
| `reserve`、`capacity`、`length`、`empty`、`clear` | 容量与状态 |

### `PriorityQueue<T, Cmp = Less<T>, A = DefaultAllocator>`

`Heap` 的包装，接口是 `push`/`pop`/`top`/`length`/`empty`/`clear`。默认是优先取出
最小的；`PriorityQueue<int, Greater<int>>` 优先取出最大的。

## 数学：`std.math`

### 对任何数字都成立

```ft
std.min(a, b)             // 较小的
std.max(a, b)             // 较大的
std.clamp(v, low, high)   // 夹到 [low, high]
std.abs(v)                // 去掉符号；无符号类型原样返回
std.square(v)             // v * v
std.squared_length(a, b)  // a*a + b*b
```

它们是模板，只要求类型支持 `<`、`-`、`*`，所以 `int`、`f64`、`String`（有
`<`）都可以用，而且不产生运行时代价。

### 实数

这些是 C 库的函数，参数与返回值都是 `f64`：

| 函数 | 结果 |
| ---- | ---- |
| `sqrt(x)` | 平方根 |
| `pow(base, exponent)` | 幂 |
| `floor(x)`、`ceil(x)`、`round(x)` | 向下 / 向上 / 最近的整数 |
| `fabs(x)` | 绝对值 |
| `exp(x)`、`log(x)` | `e` 的幂、自然对数 |
| `sin(x)`、`cos(x)`、`tan(x)` | 三角函数，弧度 |
| `asin(x)`、`acos(x)`、`atan(x)` | 反三角函数，弧度 |
| `atan2(y, x)` | 点 `(x, y)` 的角度 |

`f32` 要显式 `as f64` 传进去：语言没有隐式数值转换，这里的函数也不收窄。

## 随机数：`std.random`

`std.random` 是程序共享的一个 `Random`，也就是 MT19937（梅森旋转）：624 个字的状
态、周期 2^19937-1，同一种子给出的序列与 C++ `std::mt19937` 一致。它在 `__setup__`
时用系统的真随机数（POSIX 的 `/dev/urandom`、Windows 的 `BCryptGenRandom`）播种，
所以两次运行不同；要可重复就在取数前 `seed`。

```ft
n u32 = std.random();                 // 32 位随机数
die int = std.random.uniform(1, 6);   // 1..=6，两端都含
```

| 成员 | 说明 |
| ---- | ---- |
| `std.random()` | 下一个 32 位字 |
| `seed(n)` | 重新播种；同一种子同一序列 |
| `seed_from_entropy()` | 用系统熵重新播种；返回是否成功（`except`） |
| `uniform(low, high)` | 实参类型决定分布：整数类型取闭区间 `[low, high]` 且无模偏差（要求 `high >= low`）；浮点类型取 `[low, high)` |
| `uniform_real(low, high)` | `[low, high)` 的 `f64` |
| `normal(mean, deviation)` | 正态分布，Box-Muller |
| `bernoulli(chance)` | 以 `chance` 的概率为 `true` |
| `exponential(rate)` | 指数分布，均值 `1/rate` |
| `lognormal(mean, deviation)` | 对数正态（正态取 `exp`） |
| `poisson(mean)` | 泊松分布；小均值用 Knuth，大均值用正态近似 |
| `binomial(trials, chance)` | 二项分布（逐次伯努利，线性于 trials） |
| `geometric(chance)` | 首次成功所需的试验次数，均值 `1/chance` |
| `choice(&values)` | 等概率选一个元素 |
| `weighted_choice(&values, &weights)` | 按权重选一个元素 |
| `shuffle(&array)` | 就地洗牌 `std.Array<T>`（固定数组先转成 Array），Fisher-Yates，每种排列等概率 |

`Random` 也负责 `__call__`，所以 `std.random()` 就是一次取数；需要独立流的程序自己
声明 `rng Random = Random(seed)`，不干扰全局流。系统熵源在
`std/internal/random_entropy.ftm` 和它的平台文件 `std/internal/random_entropy.windows.ftm` 里。

## 操作系统：`std.os`

```ft
import std.os;

os.pid();                 // 进程 ID
os.getenv("HOME");        // 环境变量；没有时抛 KeyError（has_env 是问句形式）
os.set_env("LOG", "1");   // 只改本进程的环境
os.exit(0);               // 结束进程：之后的代码、析构、finally 都不再运行
os.cwd();                 // 当前工作目录，返回 Path
os.chdir(path);           // 换目录
os.temp_dir();            // TMPDIR，没有时用平台默认
os.home();                // HOME / USERPROFILE，没有时是当前目录
os.monotonic_ns();        // 单调时钟，纳秒；两次读数之差是时长
os.sleep_ms(10);          // 挂起当前线程
os.platform();            // "linux" / "windows" / "macos"：构建目标，运行期取
os.arch();                // "x86_64" / "aarch64"：同上
```

`std.os` 管进程与系统；文件、路径分别属于 `std.io`、`std.fs`。`os.platform()` 和
`os.arch()` 返回**构建目标**，但它们是普通函数调用（返回 C 字符串）：要编进
`if constexpr`/`static_assert` 就用宏 `$meta.PLATFORM` / `$meta.ARCH`。
`os.platform()` 与字面量比较要经 `C.strcmp`，因为它是 `*const char`。

## 跨平台

同一份接口、每平台一份实现：文件命名成 `名字.<平台>.ftm`，加载器按目标平台优先读它
（见 [08 模块](language/08-modules.zh.md#模块在哪里找)）。线程、互斥量、条件变量在 `std/internal/sync_sys.ftm`（glibc）、`std/internal/sync_sys.macos.ftm`
（Darwin，互斥量要 `pthread_mutex_init`，零字节不是初始值）与
`std/internal/sync_sys.windows.ftm`（Windows，SRW 锁 + 条件变量 + `CreateThread`）之间切换；系统熵源在 `std/internal/random_entropy.ftm` /
`std/internal/random_entropy.windows.ftm`；进程与环境在 `std/internal/os_sys.ftm` / `std/internal/os_sys.macos.ftm` /
`std/internal/os_sys.windows.ftm`。
`os.platform()`、`$meta.WINDOWS` 和 `if constexpr` 让一份程序里只留下当前平台的
那半代码。

编译器本身也这样跨平台：目标是它运行所在的系统，链接由平台的 C 编译器驱动
（`CC` / `FALLC_LINKER`，Windows 默认 `clang`），Windows 上默认输出加 `.exe`，
位置无关代码只在 POSIX 上要。Windows 构建需要 PATH 上有 LLVM 或 MinGW 工具链
（链接参数是 GNU 风格）。`FALLC_TARGET_OS` 可以覆盖**检查**用的目标平台：在 Linux 上
用 `FALLC_TARGET_OS=windows fallc --check prog.ft` 就能把 Windows 那套库文件完整地
解析、类型检查一遍（不链接，不运行）。

## I/O：`std.io`

### 一个 `File`，按终端的方式

`std.io` 只有一个类型 `File`：一个 C 流加上打开方向。标准流、磁盘文件、别的 C 库交过来的流，全都是它。

```ft
std.out    File   // 标准输出：File(C.stdout, OpenMode.WRITE)
std.err    File   // 标准错误
std.in     File   // 标准输入：File(C.stdin, OpenMode.READ)
```

能不能读、能不能写是**运行期**问的，和终端一样：

```ft
std.out.readable()   // false
std.out.writable()   // true
```

所以在只读文件上写不是编译错误，而是运行期抛 `IOError`。代价是方向检查推到了运行期，换来的是一个类型就能对待所有句柄。

### 打开

```ft
f    File = File("notes.txt", OpenMode.WRITE)??;   // "wb"，截断
src  File = File("notes.txt", OpenMode.READ)??;    // "rb"
both File = File("notes.txt", OpenMode.RW)??;      // "r+b"，文件必须存在
```

`OpenMode` 就是 C 的那三种。`File()` 是没有流的无效句柄，`open(path, mode)` 之后才有流；构造时给路径等价于「默认构造 + `open`」。打不开抛 `IOError`。

也可以包一个已经打开的 C 流——`std.out` 自己就是这么来的：

```ft
log File = File(C.stderr, OpenMode.WRITE);
```

| 查询 | 含义 |
| ---- | ---- |
| `valid()` | 背后有流 |
| `readable()` | 打开模式允许读，并且有流 |
| `writable()` | 打开模式允许写，并且有流 |

### 写

```ft
std.out.print("x = ", 42, ", y = ", 2.5);   // 每个参数写成 str(参数)，不加分隔
std.out.write("text");                 // 不换行
std.out.put('x');                      // 一个字符
std.out.flush();
```

`write` 有三个重载：`&const String`、`*const char`、`(*const byte, usize)`，都返回写入的字节数。`put` 返回 `1`。

### 格式化：`print`、`println`、`input`

三个具名成员负责格式化，名字本身就说明了方向：

```ft
std.out.print("x = ", 42, '\n');   // 每个实参 str() 之后依次写
std.out.println("done");            // 同上，再加一个换行
std.in.input(n, word);              // 每个引用实参读一个词，存入 __input__
```

- `print` / `println` 收**值**：文本是文本，数字是它的十进制，字符是它自己，声明了 `__str__` 的类型是它返回的文本。参数个数不限（形参包，见 [`language/04-functions.zh.md`](language/04-functions.zh.md#形参包)）。
- `input` 收**引用**：一个元素就是调用者的变量，读到的词经该类型的 `__input__` 写进去；解析失败抛 `ValueError`，输入提前结束抛 `EOFError`。
- 因为方向由方法名决定，“能写”只要求 `str` 能渲染，“能读”只要求声明了 `__input__`——两者在**编译期**各自检查，不再有运行期分支。所以一个只声明了 `__str__` 的类型可以 `print`，但 `input` 它是编译错误。
- 第一个实参是**格式串**：`{}` 依次换成后面的值，`std.print("{} + {} = {}\n", 1, 2, 3)`；
  没有 `{}` 时沿用旧的“逐个 `str` 后拼接”。
- 花括号里可以写**格式规格** `{:spec}`，规则由 `std/fmt.ftm` 实现（`std.io` 的
  `print` 就是它加一个文件）：`[[fill]align][width][.precision][type]`——
  `{:>8}` 右对齐、`{:<8}` 左对齐、`{:^8}` 居中、`{:08}` 补零、`{:.2f}` 两位小数、
  `{:x}`/`{:X}`/`{:b}`/`{:o}` 进制转换、`{:08.2f}` 组合。
- 类型自己决定文本：声明 `__format__` 的类型由它渲染（`Formattable` 要求），否则用 `str`。

两个**短形式**（自由函数，等价于对标准流调用上面的成员）：

```ft
std.print("x = ", 42, '\n');   // 等价于 std.out.print(..)
line String = std.input();       // 等价于 std.in.getline(&line)：返回整行
```

`std.input()` 返回去掉行尾的 `String`；输入结束抛 `EOFError`（空行返回空串，不算结束）。


### 读

```ft
c char = std.in.get();          // 读一个字符，EOF 时抛异常
c char;
if (std.in.get(&c)) { }        // 读到返回 true，EOF 返回 false
c char = std.in.peek();         // 看一眼，不取走；EOF 时抛异常
if (std.in.eof()) { }           // 下一个字符就是结尾
```

`get`/`peek` 在结尾时**抛异常**，`get(&char)` 是检查式的版本：结尾返回 `false`。`eof()` 看的是「下一个字符」，不是「上次读失败了」。

读一整行用 `getline`：

```ft
line String;
while (std.in.getline(&line)) {
    std.out.println(line);
}
std.in.getline(&line, ';');     // 换一个终止字符
```

`getline` 先**清空**目标（它回答的是「下一行是什么」，不是「这一行还剩下什么」），读到的终止字符被消费掉、不属于文本，没读到任何东西时返回 `false`。

### 定位

`seek(offset, from)`，`from` 是 `SeekFrom.START`/`CURRENT`/`END`，返回新位置；终端和管道上抛 `IOError`。用 `OpenMode.RW` 打开的文件，写完之后 seek 回去就能读到刚写的内容。`close` 在标准流上只忘记句柄，不会真的关掉 C 的流。

## 同步：`std.sync`

### `Mutex` 与 `Lock`

```ft
m Mutex;
{
    guard Lock = &m;     // 构造时加锁
    ...
}                        // 析构时解锁
```

`Mutex` 可以直接 `lock()`/`unlock()`，但推荐用 `Lock`：它把解锁绑在作用域上。
`Mutex` 是值，不是全局的；要共享就把它的地址传给线程。

### `CondVar`

```ft
ready CondVar;
m Mutex;
guard Lock = &m;
while (!ready_now) {
    ready.wait(&m);      // 等待期间释放锁，醒来前重新加锁
}
ready.notify_one();      // 或 notify_all()
```

### `Thread` 与 `sleep`

```ft
fn work(argument *byte) u32 {
    std.sleep(10);       // 毫秒
    return 0;            // 线程的退出值；`join` 目前不收集它
}

t Thread = Thread(work, nullptr)??;         // 从函数指针构造，可以带一个参数
t.join();
```

`Thread` 的构造与 `join` 都可能抛 `ThreadError`。线程入口的类型是
`*fn(*byte) u32`，参数通过 `*byte` 传递，返回的 `u32` 是线程的退出值——但 `join`
现在返回 `void`，这个值还不被收集。

### `Atomic<T>`

```ft
count Atomic<int> = 0;
count.fetch_add(1);
count.store(7);
v int = count.load();
```

`load`/`store`/`fetch_add`/`fetch_sub`/`exchange`/`compare_exchange` 都是原子操作，
每个都接受一个可选的内存序 `AtomicOrder`（`RELAXED`/`ACQUIRE`/`RELEASE`/`ACQ_REL`/
`SEQ_CST`），**默认 `SEQ_CST`**：

```ft
count.store(7, AtomicOrder.RELEASE);
v int = count.load(AtomicOrder.ACQUIRE);
count.fetch_add(1, AtomicOrder.ACQ_REL);
```

**当前实现在内部用一把互斥锁**：它的 lock/unlock 本来就是顺序一致的，所以任何序都
至少按要求的强度执行，参数先被校验、等真正的原子实现时再按它映射；并发量高时并不比
加锁快。真正的无锁实现要等语言层面的原子内建。

### 读多写少、信号量、一次性与屏障

`std.sync` 还提供四个原语（[`std/sync.ftm`](../../standard/std/sync.ftm)）：

| 原语 | 用法 |
| ---- | ---- |
| `RwLock` | `lock_shared()`/`unlock_shared()` 读锁（多个读者可同时持有）；`lock()`/`unlock()` 写锁（独占）。等待中的写者会挡住新读者，避免写者饿死 |
| `Semaphore(initial)` | `acquire()` 取一个许可（没有时等待），`release()` 归还一个；`available()` 查看剩余 |
| `Once` | `call(&action)` 保证 `action` 只执行一次，其余调用者等它执行完 |
| `Barrier(parties)` | `wait()` 在屏障处等所有方到齐，然后一起继续 |

```ft
import std.sync;

bar Barrier = Barrier(2u32);
once Once = Once();
once.call(&setup);          // 只跑一次
bar.wait();                 // 等到 2 方都到
```

`RwLock`/`Semaphore`/`Barrier` 用 `Mutex`+`CondVar` 实现；`Once` 的状态是
0/1/2（未开始/执行中/完成），别的调用者等状态变成 2。

## 并发：`std.concurrent`

三件事（[`std/concurrent.ftm`](../../standard/std/concurrent.ftm)）：

- **`Promise<T>` / `Future<T>`**：一个线程产出值，另一个线程等它。`Promise` 只能
  `set` 一次（第二次是空操作），`Future.get()` 会等到值到达；`future.ready()` 不等待。
  两边共用一个带引用计数的控制块，谁先死都行。
- **`Channel<T>`**：队列式传值。`send` 追加并唤醒读者；`recv` 在空队列上等待，通道
  关闭且空时抛 `ChannelClosed`；`try_recv()` 返回 `?T` 不等待；`close()` 停写并唤醒
  所有等待者。
- **`ThreadPool`**：固定数量的工作线程从队列取任务。构造后必须 `start()` ——
  构造函数跑在临时对象上，线程若拿着临时地址会读到搬走后的对象；`submit(task, arg)`
  投递，`shutdown()` 让队列跑完后停止。

```ft
import std.concurrent;

channel Channel<int> = Channel<int>();
producer_thread = Thread(producer, &context as *byte)??;
value int = channel.recv()??;

promise Promise<int> = Promise<int>();
future Future<int> = promise.future();
result int = future.get()??;

pool ThreadPool = ThreadPool(4usize);
pool.start()??;
pool.submit(task, &state as *byte);
pool.shutdown()??;
```

任务函数是 `*fn(*byte) u32`，参数由调用者分配并保证活到任务跑完；线程入口与
`std.sync.Thread` 一致。

## 时间：`std.time`

三种问题三种类型（[`std/time.ftm`](../../standard/std/time.ftm)）：`Duration` 是一段
时间（纳秒），`Instant` 是**单调时钟**上的一个点（只用于测量经过多久），
`SystemTime` 是**墙钟**上的一个点（Unix 纪元纳秒，用来记录"什么时候"）。

```ft
import std;
import std.time;

start Instant = time.now();
// ... 工作 ...
took Duration = time.since(&start);
std.print("{} {} {}\n", took.nanos(), took.millis(), took)??;  // 0.005s

d Duration = time.from_millis(1500);   // from_nanos / from_seconds 同理
stamp SystemTime = time.unix_now();
dt DateTime = stamp as DateTime;
std.print("{}\n", dt)??;   // 2026-10-20 16:54:58
```

`Duration` 支持 `+`/`-`/比较与文本（`1.500s`）；`SystemTime` 经 `as` 转为
`DateTime`（`year()`/`month()`/`day()`/`hour()`/`minute()`/`second()` 与
`YYYY-MM-DD HH:MM:SS` 文本）；`time.sleep(&duration)` 睡眠。平台差异在
`std/internal/time_sys.ftm`（POSIX `clock_gettime`）与 `std/internal/time_sys.windows.ftm`
（`GetSystemTimeAsFileTime`）。

## 网络：`std.net`

`std.net` 提供 IPv4 TCP（[`std/net.ftm`](../../standard/std/net.ftm)）：
`connect(&"127.0.0.1", port)`/`listen(&host, port, backlog)`（port 0 由系统分配，
`local_port()` 查回）、`accept`、`write_all(&bytes)`、`read_into(&bytes, max)`、
`read_text(max)`；`TcpStream`/`TcpListener` 的 `__drop__` 会关闭套接字，也可显式
`close()`。句柄统一为 `i64`（POSIX fd / Windows SOCKET），平台后端在
`std/internal/net_sys.ftm`/`std/internal/net_sys.windows.ftm`。只支持点分四段地址与 `localhost`：DNS、IPv6、
UDP 是独立工作，本模块明确不做，而不是悄悄只处理一部分。

## 数值、测试与调试：`std.math` 扩展、`std.complex`、`std.matrix`、`std.test`、`std.debug`、`std.prelude`

- **math 扩展**（[`std/math.ftm`](../../standard/std/math.ftm)）：`hypot`/`cbrt`/`trunc`/`fract`/
  `sign`/`signum`/`lerp`/`degrees`/`radians`/`log2`/`log10`/`sinh`/`cosh`/`tanh`/`asinh`/
  `acosh`/`atanh`/`is_nan`/`is_infinite`/`is_finite`，带 `IsInteger` 约束的
  `gcd`/`lcm`/`isqrt`/`is_even`/`is_odd`/`is_power_of_two`。实数常量以函数给出
  （`pi()`/`e()`/`tau()`/`infinity()`/`nan()`）：模块常量必须是常量表达式，实数
  字面量不是。
- **complex**（[`std/complex.ftm`](../../standard/std/complex.ftm)）：`Complex {re, im}` 值类型，
  `add`/`sub`/`mul`/`div`（除零报错）/`conjugate`/`abs`/`arg`/`norm_squared`/`pow`、
  `==`/`!=`、`__str__`（`a+bi`）。
- **matrix**（[`std/matrix.ftm`](../../standard/std/matrix.ftm)）：`matrix`/`zeros`/`identity`、
  `rows`/`columns`/`at`/`set`、`add`/`sub`/`mul`（维度检查）/`scale`/`transpose`、
  `==`/`!=`、`__str__`。**不提供求逆与分解**：没有主元选择与容差的逆矩阵是错的，
  模块明确说明而不是近似实现。
- **test**（[`std/test.ftm`](../../standard/std/test.ftm)）：`$CHECK`、`$CHECK_EQ`、
  `$CHECK_EQ_REAL`、`$CHECK_EQ_TEXT`、`$FAIL` 宏；宏把条件文本与
  `$__builtin_FILE`/`$__builtin_LINE` 传给断言函数，失败抛 `TestError` 并带位置。
  断言宏以 `??` 结尾，因此用在 `except` 函数（测试 `main`）里。
- **debug**（[`std/debug.ftm`](../../standard/std/debug.ftm)）：`$ASSERT_DEBUG`、
  `$DEBUG_UNREACHABLE`、`print_trace(&String)`（写 stderr，不依赖 io 层）、
  `breakpoint()`（`abort`）。注意宏名是 `ASSERT_DEBUG`：`DEBUG_ASSERT` 这个名字
  在宏注册时不可见（开发中发现并规避）。
- **prelude**（[`std/prelude.ftm`](../../standard/std/prelude.ftm)）：常用模块的重导出。
  语言的导入规则要求**函数仍写模块名**（`prelude.print`），类型（如 `String`）可直接写；
  prelude 的价值是一个 import 拿到常用类型与入口，而不是改变名字查找规则。

## 校验、哈希、UUID 与抽样：`std.crc`、`std.crypto`、`std.uuid`、`std.rand_dist`

- **crc**（[`std/crc.ftm`](../../standard/std/crc.ftm)）：`crc8`（poly 0x07，初值 0）、
  `crc16`（CCITT-FALSE，poly 0x1021，初值 0xffff）、`crc32`（ISO-HDLC，即 PNG/zip 的
  CRC-32），测试用标准向量 `"123456789"`。
- **crypto**（[`std/crypto.ftm`](../../standard/std/crypto.ftm)）：`sha256`/`sha256_hex`、
  `hmac_sha256`/`hmac_sha256_hex`、`hex`；按规范实现并用标准向量（`"abc"`、空串、
  RFC 4231）验证。明确不是密码学库：没有常量时间比较、密钥管理；SHA-1/MD5 不提供。
- **uuid**（[`std/uuid.ftm`](../../standard/std/uuid.ftm)）：`Uuid()??` 随机 UUID（版本位与
  RFC 4122 变体位已设置）、`as String`/`__str__`、`parse_uuid(&text)??`、`version()`、
  `variant()`、`==`/`!=`；两个 `u64` 存储，复制/比较/返回都不涉及所有权。
- **rand_dist**（[`std/rand_dist.ftm`](../../standard/std/rand_dist.ftm)）：在
  `std.random.Random` 之上的分布函数名——`sample_uniform`/`sample_uniform_real`/
  `sample_normal`/`sample_exponential`/`sample_lognormal`/`sample_poisson`/
  `sample_binomial`/`sample_geometric`/`sample_bernoulli`。带 `sample_` 前缀是因为
  自由函数若以接收者为第一个参数会被方法语法命中，与 `Random` 的同名方法冲突
  （开发中真的导致过自递归）；`choice`/`weighted_choice` 仍是 `Random` 的方法。

## 系统接口：`std.process`、`std.term`、`std.dynlib`、`std.signal`、`std.compress`、`std.regex`

- **process**（[`std/process.ftm`](../../standard/std/process.ftm)）：`spawn(&program, &arguments)`
  启动子进程（`argv[0]` 自动补程序名），`wait()` 等待并返回退出码；POSIX 后端
  `fork`+`execvp`（被杀的子进程按 shell 习惯返回 `128+signal`），Windows 后端
  `CreateProcessA`。
- **term**（[`std/term.ftm`](../../standard/std/term.ftm)）：ANSI SGR——`color_fg`/`color_bg`
  八色、`style_bold`/`style_dim`/`style_underline`/`style_reset`；`supports_color()`
  在 `NO_COLOR`、`TERM=dumb` 或输出不是终端时返回 false。
- **dynlib**（[`std/dynlib.ftm`](../../standard/std/dynlib.ftm)）：`open(&path)` 得到
  `Library`，`symbol(&name)` 得到 `*byte` 地址（调用者转成自己的函数指针类型），
  `close()`；POSIX `dlopen`/`dlsym`，Windows `LoadLibraryA`/`GetProcAddress`。
- **signal**（[`std/signal.ftm`](../../standard/std/signal.ftm)）：`Signal` 枚举（HUP/INT/TERM/USR1）
  与 `install`/`restore`/`signal_self`，底层是 C 运行时的 `signal`。
- **compress**（[`std/compress.ftm`](../../standard/std/compress.ftm)）：`adler32`、zlib
  **stored 块**的 `deflate_stored`/`inflate_stored`；其它 deflate 块类型明确报
  `ValueError`，不做猜测性解码。
- **regex**（[`std/regex.ftm`](../../standard/std/regex.ftm)）：回溯匹配器，支持字面量、
  `.`、`[a-z]`/`[^0-9]`、`\d`/`\w`/`\s`、`^`/`$`、`*`/`+`/`?`；组、选择、
  反向引用、懒惰量词、`{n,m}` 在 `compile` 时明确报错。`find` 返回 `?Match`
  （`start`/`end`/`length`/`matched`），`is_match` 返回 bool。

## 表格与配置：`std.csv`、`std.toml`、`std.xml`

- **CSV**（[`std/csv.ftm`](../../standard/std/csv.ftm)）：`parse_rows(&text)` 返回
  `Array<Array<String>>`，`write_rows(&rows)` 写回。规则：逗号分隔、引号字段里逗号/
  换行/引号按字面，`""` 表示一个引号；不合法（引号未闭合、未加引号的字段中间有引号）
  抛 `ValueError`。写回时只在需要时加引号。
- **TOML**（[`std/toml.ftm`](../../standard/std/toml.ftm)）：`parse_toml(&text)` 给
  `TomlDocument`（文件顺序的扁平条目表）。支持注释、`[table]`、`key = value` 与
  字符串/整数/实数/布尔；数组、内联表、日期、多行字符串明确不支持（报 `ValueError`）。
  `get(&table, &key)` 返回 `?TomlValue`，值用 `as String`/`as i64`/`as f64`/`as bool` 读出。
- **XML**（[`std/xml.ftm`](../../standard/std/xml.ftm)）：`parse_xml(&text)` 给
  `XmlDocument`（节点数组 + 子节点下标，和 JSON 同样的所有权形状）；`XmlValue` 句柄
  提供 `name`/`text`/`child_count`/`child(i)`/`attribute_count`/`attribute_name`/
  `attribute_value`/`attribute(&key)`；`serialize` 写回。支持元素、属性、文本、五个
  命名实体与数字实体，跳过注释与 XML 声明；CDATA/DTD 不支持。

## JSON：`std.json`

`std.JsonDocument` 拥有整棵解析树（所有节点在一个 `Array<JsonNodeData>` 里，子节点用
下标表示），`JsonValue` 只是"文档 + 下标"的小句柄，所以复制/传递很便宜；句柄不能比
文档活得久（[`std/json.ftm`](../../standard/std/json.ftm)）：

```ft
document JsonDocument = std.parse(&text)??;
root JsonValue = document.root();
key String = String("name");
name String = root.member(&key).value()?? as String??;
written String = std.serialize(&root)??;
```

`kind()` 返回 `NULL_VALUE`/`BOOLEAN`/`NUMBER`/`TEXT`/`ARRAY`/`OBJECT`；数组用
`length()`/`at(i)`，对象用 `length()`/`member(&key)`（`?JsonValue`，缺键是"没有"而不是
失败）/`member_key(i)`/`member_value(i)`；标量用 `as Bool`/`as f64`/`as String` 读出，
类型不对抛 `ValueError`；`is_null()`。解析失败同样抛 `ValueError`。数字统一是 `f64`。

## 编码与文本工具：`std.encoding`、`std.unicode`、`std.strings`

**编码**（[`std/encoding.ftm`](../../standard/std/encoding.ftm)）把字节序列变成可传输的
文本，输入输出都是 `Bytes`（字节不一定是文本）：

```ft
hex String = std.hex_encode(&data);        // "4142"
back Bytes = std.hex_decode(&hex)??;        // 解码失败抛 ValueError
b64 String = std.base64_encode(&data);      // "QUI="
pct String = std.percent_encode(&text);     // URL：字母数字 -_.~ 保留，其余 %XX
```

**Unicode**（[`std/unicode.ftm`](../../standard/std/unicode.ftm)）：`encode(码点)` 给
UTF-8 字节（拒绝 >U+10FFFF、代理区），`decode(&text)` 给 `Array<u32>`（非法 UTF-8
抛 `ValueError`），`valid`/`count` 分别是"是否合法"和"码点个数"；`next_code_point`
返回 `Pair<码点, 字节数>`。

**文本工具**（[`std/strings.ftm`](../../standard/std/strings.ftm)）：`join(&parts, &sep)`、
`repeat`、`replace_all`、`lines`、`pad_start`/`pad_end`、`upper`/`lower`（ASCII）。

## 区间工具：`std.iter` 与 `std.range`

`std.iter`（[`std/iter.ftm`](../../standard/std/iter.ftm)）是循环里对一对位置做的
小动作，全部按迭代器类型单模板：`advance`/`skip`/`take`/`nth` 移动位置，
`find_if`/`count_if`/`all_of`/`any_of`/`none_of` 用谓词（谓词是一个有 `__call__` 的
类型，如内置的 `Even`/`Odd`）：

```ft
first *int = std.find_if(values.begin(), values.end(), even);
n usize = std.count_if(values.begin(), values.end(), odd);
cursor *int = std.advance(values.begin(), values.end(), 2usize);
```

`std.range`（[`std/range.ftm`](../../standard/std/range.ftm)）给整数区间：
`to(n)`（0..n）、`up_to(low, high)`、`down_to(low, high)`、`step_range(low, high, step)`
（步长为 0 抛 `ValueError`）。`Range` 自己既是容器又是位置，所以能直接
`for (value i64 : std.to(5i64))`；终点**不包含**，步长不整除时到越过终点为止。

## 网格：`std.grid`

`Grid<T>`（[`std/grid.ftm`](../../standard/std/grid.ftm)）是行主序的二维数组：一块
`Array<T>`，元素 `(row, column)` 在下标 `row * columns + column`。构造要一个填充值
（元素类型不一定有默认构造）：

```ft
grid Grid<int> = Grid<int>(2usize, 3usize, 0);
grid.set(1, 2, 7);
v int = grid.at(1, 2);
rows usize = grid.rows();
```

`rows`/`columns`/`length`/`empty`、`at`（可写/只读）、`set`、`fill`、`==`；
`at` 越界 panic。

## 日志：`std.log`

一条日志是 `[LEVEL] 文本`，默认写到标准错误；级别过滤在**构造文本之前**发生，所以被
关掉的 `debug` 不花代价（[`std/log.ftm`](../../standard/std/log.ftm)）：

```ft
import std;
import std.log;

log.info("starting with {} items", count)??;   // [INFO] starting with 3 items
log.set_level(log.Level.DEBUG);
log.debug("value = {}", value)??;
log.set_level(log.Level.OFF);                  // 全关
```

级别是 `TRACE`/`DEBUG`/`INFO`/`WARN`/`ERROR`/`OFF`；`set_level`/`level`/`enabled`/
`level_name` 可查；`set_target(&file)` 把日志改写到别的流。文本规则与
`std.print` 相同（`{}` 依次填后面的值，级别器自己补换行）。并发写不在这里加锁：
需要整行不交错的多线程程序在自己的互斥量里调用。

## 文件路径：`std.fs`

```ft
p Path = "/home/user/notes.txt";
p.directory();     // "/home/user"
p.file_name();     // "notes.txt"
p.extension();     // "txt"
p.join(other);     // 拼接，必要时补分隔符
p.exists();        // 当前文件系统上是否存在
p.file(OpenMode.READ);  // 打开这个路径命名的文件，得到一个 std.File
```

`Path` 拥有自己的文本（内部是 `String`），所以拷贝、移动、赋值、析构都齐全；
`text()` 给 `*const char`，`data()` 给 `&const String`，`__str__` 也给 `String`。
`directory`/`file_name`/`extension` 只做字符串切分，**不访问文件系统**，也不检查
路径是否合法；`exists()` 才会打开文件看一眼。`file(mode)` 是路径与文件的合并写法：
它把 `p.text()` 交给 `std.File` 的构造函数，打不开时抛和构造函数一样的 `IOError`。

## 编译期信息：`meta`

`meta` 不属于 `std`：它是编译器提供的宏，单独 `import meta;` 才有。

```ft
import meta;
std.out.println("built from ", $meta.FILE, " on ", $meta.PLATFORM);
if ($meta.WINDOWS) @constexpr { ... } else { ... }
```

| 宏 | 展开成 |
| -- | ------ |
| `$meta.FILE` | 当前文件的路径，字符串 |
| `$meta.LINE` / `$meta.COLUMN` | 宏所在的行号 / 列号，整数 |
| `$meta.PLATFORM` | 目标平台：`linux`、`windows`、`macos` |
| `$meta.ARCH` | 目标架构：`x86_64`、`aarch64` |
| `$meta.WINDOWS` / `$meta.POSIX` | `1` 或 `0`，可直接用于 `if constexpr` |
| `$meta.DATE` / `$meta.TIME` | 构建日期 `YYYY-MM-DD` 与时间 `HH:MM:SS`（UTC） |

宏在解析前展开，所以能出现在任何记号位置；它们由编译器的内建宏（`$__builtin_FILE` 等）
包装而来，实现细节不在语言里。平台相关的库实现放在 `名字.平台.ftm` 文件里，加载器
按目标平台优先选择它，见 [08 模块](language/08-modules.zh.md#模块在哪里找)。

## 限制

- **`Atomic<T>` 不是无锁的。** 它用互斥锁实现，见上。
- **哈希容器没有顺序保证，树容器没有重复键的稳定顺序。** 见各自小节。
- **`Path` 不解析 `..` 与 `.`。** 它只按分隔符切分，路径规范化是另一个操作。
- **`File` 的方向是运行期判断的。** 在只读文件上写、在只写文件上读、在终端上 seek 都是运行期异常；需要编译期方向就自己包一层类型。
