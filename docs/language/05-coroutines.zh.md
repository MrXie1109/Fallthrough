# 05 协程

> [English](05-coroutines.md)

## 范围

本章规定协程：可以挂起自己、把一个值交给恢复它的人、并在被恢复时继续执行的函数。它规定
声明协程的属性、协程返回的结果类型、`yield` 表达式、调用方运行它所用的两个成员，以及协程
函数体如何成为一个状态机。

## 声明协程

### 形式

```ft
import std;

@coroutine
fn echo(n int) std.Coroutine<int, int, int> {
    i int = 0;
    while (i < n) {
        i += yield i;
    }
    return i;
}
```

2.1 标了 `@coroutine` 的函数，返回类型**必须**是 `std.Coroutine<Send, Yield, Return>`。
其他任何返回类型都是错误。

2.2 三个类型参数是协程的约定，各自回答一个不同的问题。任何两个都不是第三个的推论。

| 类型 | 它是什么 | 它出现在哪里 |
| ---- | -------- | ------------ |
| `Send` | `resume` 送进去的值，也是 `yield` 表达式的值 | `resume` 的实参；`yield` 表达式的值 |
| `Yield` | `yield` 交出去的值，也是 `resume` 回答的值 | `yield` 的操作数；`resume` 返回的 optional |
| `Return` | 函数体 `return` 给出的值，也是 `result` 回答的值 | `return` 的操作数；`result` 返回的值 |

2.3 函数体的 `return` 给出的是 `Return` 类型，而不是协程对象：上例的 `echo` 返回 `int`，
而对象是**调用** `echo` 得到的东西。

2.4 三者都可以是 `void`，每个 `void` 只表示那一个位置上“没有值在传递”：

| 位置 | `void` 的含义 |
| ---- | ------------- |
| `Send` | 协程不接收值；`resume` 不带实参调用 |
| `Yield` | `yield` 不交出值；`resume` 不回答值 |
| `Return` | 函数体不返回值；`result` 不回答值 |

2.5 协程不接受改变其形态的其他属性，也不能标 `@extern_C`：协程对象是语言的值，不是 C ABI
的类型。

## `yield` 表达式

3.1 `yield expr` 把 `expr` 交给恢复者，并产出**下一次** `resume` 送进的 `Send`。它是表达
式，因此可以出现在任何表达式可以出现的位置，其值照常使用：

```ft
i += yield i;
```

3.2 操作数按 `Yield` 检查，规则与 `return` 的值按 `Return` 检查完全相同：表达式不是该类型
时由该类型自己的构造函数接手，否则转换到该类型。因此在 `Yield` 为某个带 `__init__` 的类型
的协程里，`yield 300` 会调用该构造函数，正如 `return 300` 在那个类型的函数里一样。

3.3 不带操作数的 `yield;` 不交出任何值，仅在 `Yield` 为 `void` 时允许。它仍是一个挂起点：
协程停在这里，之后的一次 `resume` 继续它。

3.4 在 `@coroutine` 函数体之外写 `yield` 是错误。

3.5 一个挂起点可以被多次经过。循环里的 `yield` 每转一圈挂起一次，每次 `resume` 都从停下
它的那个 `yield` 之后继续，而不是从第一个之后。

3.6 一个协程的挂起点数量有上限，超过上限时在编译期报错。上限由实现决定，**必须至少为
1024**；本实现的上限是 1024。

## 运行协程

4.1 调用 `@coroutine` 函数会创建协程并把它返回。**此时函数体不会运行。** 这次调用分配帧、
把实参存进帧，仅此而已。

4.2 第一次 `resume` 就是启动：它从函数体开头运行，停在第一个 `yield`。没有需要先单独调用的
东西，也没有「开始」这一步——`resume` 是运行协程的唯一方式。

4.3 `resume` 接收什么、回答什么是两个独立的问题。`Send` 决定前者，`Yield` 决定后者；协程可以
只交出值而不接收值，也可以只接收值而不交出值。四种组合是：

| `Send` | `Yield` | `resume(v)` 回答 |
| ------ | ------- | ---------------- |
| 一个类型 | 一个类型 | `?Yield` |
| 一个类型 | `void` | `bool`：协程是否挂起 |
| `void` | 一个类型 | `?Yield` |
| `void` | `void` | `bool`：协程是否挂起 |

`resume` 运行协程到下一个 `yield` 并回答该 `yield` 的值，协程结束时回答 `none`。对不接收值的
协程给实参、或对接收值的协程不给实参都是错误：调用不成形。启动协程的那次调用所送入的值不会
到达任何 `yield`：此刻没有 `yield` 在等它，因此它被丢弃。

4.4 `resume` 不抛异常。

4.5 一个协程可以等待另一个协程结束：

```
yield from inner
```

这会把 `inner` 一路恢复到结束，在此期间每一次挂起都把外层协程的轮次让出去，并回答 `inner` 的
`Return`。`inner` 必须是 `Send` 为 `void` 的协程，因为每一轮都不向它送入值；它的 `Yield` 值
（若有）被丢弃。

```ft
import std;

@coroutine
fn worker(n int) std.Coroutine<void, void, void> {
    i int = 0;
    while (i < n) {
        yield;
        i += 1;
    }
}

@coroutine
fn outer() std.Coroutine<void, void, void> {
    yield;                      // 挂起外层
    yield from worker(3);       // 随 worker 一起挂起，直到它返回
}
```

4.6 上面的序列是规范性说明。`resume` 回答协程挂起时交出的值，而它送入的值由**下一次**
`resume` 使用。这正是 `i += yield i` 读起来就是它意思的原因：第一个 `yield i` 交出 `0`，因为
`i` 仍是 `0`；第一次 `resume` 送入的 `3` 在函数体继续时才加上。

```ft
c auto = echo(12);
y ?int = c.resume(0);   // 0
y = c.resume(3);        // 3
y = c.resume(4);        // 7
y = c.resume(5);        // none
r int = c.result()??;   // 12
```

4.7 `result()` 返回函数体的 `Return` 值。它总是 `except`：对尚未结束的协程询问会抛
`NoneError`，因 `raise` 结束的协程会重新抛出该异常。`Return` 为 `void` 的协程不回答值；
`result()` 仍是 `except`，仍须处理。

4.8 `resume` 与 `result` 属于编译器。它们是协程仅有的两个成员，且都不在任何模块中声明：
协程运行的帧与状态机的状态只有构建它们的编译器知道，所以接收 `&Coroutine<..>` 的库函数体对它
无能为力。它们的形状由 `Send`、`Yield` 与 `Return` 按 4.3 与 4.7 读出。

4.9 协程对象是一个值。它可以存放在绑定里、按引用传递、从函数返回、保存在记录中；它不可拷贝，
因为两份会运行同一台状态机。

## 状态机

5.1 协程没有自己的栈。它被编译成一个函数，入口是保存在帧里的一个编号，`resume` 带着该帧再
次调用这个函数。具体地：

- **入口**保留声明给它的名字，做三件事：分配一个帧、把实参存进帧、返回一个持有该帧与机器
  地址的协程对象；
- **机器**是一个只接收帧的私有函数。它从帧里读出状态编号，跳到该编号指名的续点；状态 `0`
  是函数体的开始，每个 `yield` 各有一个续点；
- **`yield`** 把它交出的值存进帧、记下接下来该哪个续点，然后返回给恢复者；
- **`resume`** 调用机器，然后读出帧里存着的东西。

5.2 帧是一次堆分配。它存放送进来的 `Send`、载荷（`yield` 交出的值或函数体 `return` 给出的
值）、说明载荷是两者中哪一个的标记、状态编号，以及函数体每个声明绑定的槽位。

5.3 帧里存放协程自己的变量，因此它们比函数体的一次激活活得更久。`@coroutine` 函数体里声明
的绑定因此存放在帧里，而不是调用栈上：`echo` 里的 `i` 在每次 `yield` 之间都活着，而栈槽在
函数体返回给恢复者的那一刻就不存在了。

5.4 载荷槽的宽度取 `Yield` 与 `Return` 中较大者，因此较小的那个存时加宽、读时收窄。当两者
种类不同时——交出一个整数、返回一个浮点数——按位存取，读回时以读者要的类型解释，中间不做
数值转换。

5.5 帧由协程对象的析构函数释放。从未被销毁的协程会漏掉它的帧，这与语言里其他拥有资源的值的
规则相同。

## 执行器

6.1 `std.go` 运行 `Send`、`Yield`、`Return` 全为 `void` 的协程。

```ft
import std;
from std.go import go, settle;

@coroutine
fn worker(n int) std.Coroutine<void, void, void> {
    i int = 0;
    while (i < n) {
        yield;
        i += 1;
    }
}

fn main() int except {
    go(worker(3));
    go(worker(3));
    settle();
    return 0;
}
```

6.2 `go(task)` 把 `task` 追加到全局队列并返回。第一次调用会启动调度线程，任务的第一次恢复也就是
它的启动。

6.3 调度线程执行自己的循环：等待队列中出现任务，取出该任务并恢复它。`resume` 回答 `true`
时把它追加到队尾；回答 `false` 时该任务已经结束，不再放回。

6.4 使任务终结的异常被写到标准错误，该任务被丢弃。

6.5 调度线程一次运行一个任务。任务在各自的 `yield` 处交错，不并发执行，也不在调用线程上
运行。

6.6 `settle()` 等到没有任务在队列中、也没有任务在执行，然后返回自程序启动以来失败的任务数。
它自身不做任何调度。

6.7 `settle()` 返回、程序结束后，任务不再被恢复。没有取消，也没有句柄：提前结束的任务返回即可。

## 通信

7.1 `go` 不携带值。任务通过 `std.Reply<T>` 报告，该类型持有值以及值已到达这一事实：

```ft
import C;
import std;
from std.reply import Reply;
from std.go import go, settle;

@coroutine
fn compute(n int, reply &Reply<int>) std.Coroutine<void, void, void> {
    reply.set(n * 2);
}

fn main() int except {
    inbox Reply<int>;
    go(compute(21, inbox));
    settle();
    C.printf("%d\n", inbox.get());
    return 0;
}
```

7.2 `set(value)` 存下值并唤醒所有等待者。一个 reply 只携带一个值；对同一个 reply 第二次调用
`set` 会使程序中止。

7.3 `wait()` 阻塞调用线程直到值到达，然后回答该值。由 `std.go` 驱动的协程中不得调用它：调度器
只有一个线程，阻塞它的任务会使本应设置该 reply 的任务无法运行。

7.4 协程用 `yield from reply.await();` 等待，这会使它挂起直到值到达。`get()` 回答已到达的
reply 的值，未到达时使程序中止。`ready()` 回答它是否已到达。

7.5 `std.sync.Event` 只携带信号、不携带值：`set` 使 `is_set` 为真并唤醒所有等待者，`clear`
使其为假，`wait` 在它为真时返回，等待者返回不会清除它。

## 限制

8.1 `@coroutine` 函数体可以调用函数、抛异常并使用 `try`。函数体中抛出的异常存放在帧里，由
`result()` 重新抛出。

8.2 数组类型不能作为协程的 `Send`、`Yield` 或 `Return`。写出其中之一的声明会被报告。
