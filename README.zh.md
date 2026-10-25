# Fallthrough

> [English](README.md)

**本项目由 AI 生成。** 编译器、标准库、测试与本文档均由模型 **DeepSeek V4.1 Flash
1017** 在 **DeepSeek Harness** 环境中编写。人负责把握设计方向，模型负责写代码。

## 一个非常年轻的语言

Fallthrough 才刚刚开始。项目于 **2026-10-12** 开工，语言目前十分年轻，因此下面每一条都
成立：

- **语法不稳定。** 关键字、声明形式与属性仍在变动。
- **标准库不稳定。** 模块、类型、函数及其签名会随构建变化；今天存在的东西明天可能改名或删除。
- **API 与 ABI 不稳定。** 聚合体布局、调用约定以及产物的形态都还在确定中。
- **CLI 不稳定。** 选项拼写与 `--emit=` 的取值集合可能在不通知的情况下改变。
- **效率与编译速度不是最好的。** 编译器优先保证正确性而非速度：冷编译一个 import 标准库的程序
  以秒计而不是毫秒计，内存占用也没有专门优化。
- **没有兼容性承诺。** 在本节消失之前，任何一次构建都可能让程序无法编译。不要在上面构建你
  无法承担重写代价的东西。

目前，这是一个玩具编译器，但在将来，我希望将它发扬光大。

## 设计哲学

Fallthrough 是一门静态类型、编译型的系统级语言。名字本身就说明了第一条规则：`switch` 的一个
分支结束时，控制流**不会**落入下一个分支；想要落入，程序必须写 `fallthrough;`。没有写出来的
事情就不会发生。

这条规则推广为语言赖以立足的四个想法。

1. **显式优于隐式。** 程序只做它写出来的事。隐式值转换只有一份很短、可列举的清单——真值、
   数组与函数退化、加 `const`、字节指针转换；其他转换一律写成 `expr as T`。语言没有垃圾回收：
   生命周期是词法的，销毁是对作用域的说明，而不是运行时的行为。
2. **值语义与确定性析构。** 值拥有它持有的东西，并在作用域结束时通过 `__drop__` 销毁。拷贝与
   移动是类型自己的构造函数；`std.move` 是程序声明“这个具名值正在死亡”的方式。
3. **错误是类型的一部分。** 可能失败的函数写 `except`；调用点要么用 `try` 处理异常，要么用
   `?!` 把它传出去。异常携带类型名与消息。没有隐藏的错误码，也没有未检查的异常。
4. **小的可读核心，功能长在库里。** 泛型是带模板参数的类型家族，traits 在编译期按结构回答；
   容器、字符串、JSON、TOML、XML、网络与日志都是使用本语言写成的普通模块。标准库不是魔法，
   它是这门语言的第一个用户。

它想解决的问题：主流系统级语言仍然让程序员背负大量偶然复杂度——隐式转换、被静默忽略的错误、
隐藏的控制流、需要生命周期标注的所有权规则。Fallthrough 想在保留系统级语言控制力的同时，让
程序读起来就是作者实际做出的那串决定：一个构造只有一个含义，一切可见，没有隐式。

## 安装与运行

仓库地址是 [`MrXie1109/Fallthrough`](https://github.com/MrXie1109/Fallthrough)。

### 环境要求

| 要求 | 原因 |
| ---- | ---- |
| Rust 工具链（edition 2024） | 编译器是一个 Rust workspace |
| LLVM 18 开发文件 | 代码生成通过 `inkwell` 链接 LLVM |
| C 编译器 | 作为链接器驱动，以及程序链接的 C 库 |
| `make` | 构建驱动提供了短命令：`make release`、`make install` |

如果 `llvm-sys` 找不到 LLVM，用
`LLVM_SYS_181_PREFIX=/path/to/llvm-18 make release` 指定安装位置。Windows 上在
`make` 之前用 `set LLVM_SYS_181_PREFIX=C:\Program Files\LLVM` 设置同名变量。

### 克隆、编译并安装

下面这组命令是 Unix 的写法；[Windows](#windows) 另有自己的写法——安装位置、`sudo` 与
shell 配置在那边都不一样。

```sh
git clone https://github.com/MrXie1109/Fallthrough.git
cd Fallthrough
make release          # 以优化方式构建编译器
sudo make install     # 安装到 /usr/local/bin（PREFIX，默认 /usr/local）
fallc --version
```

`make install PREFIX=$HOME/.local` 不需要 root（前提是 `~/.local/bin` 在 `PATH` 中）。
卸载：`sudo make uninstall`。

`make install` 还会把标准库放到 `/usr/local/share/fallthrough/standard`，并写出一个环境
文件 `/usr/local/etc/fallthrough/env`，内容就是标准库的位置：

```sh
export FALLTHROUGH_STANDARD="/usr/local/share/fallthrough/standard"
```

编译器正是通过 `FALLTHROUGH_STANDARD` 找标准库，所以要把这一行加到自己的 shell 配置里，
让每个会话都生效。`bash` 与 `zsh`（当前用户）：

```sh
echo '. /usr/local/etc/fallthrough/env' >> ~/.bashrc   # 或 ~/.zshrc
```

`fish`：

```fish
echo 'source /usr/local/etc/fallthrough/env' >> ~/.config/fish/config.fish
```

要让整台机器的所有用户都生效：把同样一行放进 `/etc/profile`。这两条命令都不要加
`sudo`——它们改的是你自己的配置，不是 root 的。

### Windows

Windows 上没有 `/usr/local`、没有 `sudo`，也没有所有 shell 都会读的配置文件，所以安装就是
复制文件，变量写进用户注册表。

在带有 `make`、作为链接器驱动的 C 编译器和 LLVM 18 的环境里构建——MSYS2/MINGW64 的 shell，
或装了 `mingw32-make` 的 Developer PowerShell：

```sh
make release
```

然后把编译器和标准库复制到一个固定位置，并设置变量。PowerShell（当前用户）：

```powershell
$root = "$env:LOCALAPPDATA\Fallthrough"
New-Item -ItemType Directory -Force "$root\bin" | Out-Null
Copy-Item target\release\fallc.exe "$root\bin\fallc.exe"
Copy-Item standard "$root\standard" -Recurse
[Environment]::SetEnvironmentVariable(
    "FALLTHROUGH_STANDARD", "$root\standard", "User")
$env:FALLTHROUGH_STANDARD = "$root\standard"   # 当前会话
$env:PATH = "$root\bin;$env:PATH"
fallc --version
```

`cmd.exe` 里用 `setx` 永久设置：

```bat
setx FALLTHROUGH_STANDARD "%LOCALAPPDATA%\Fallthrough\standard"
```

`setx` 只对新开的终端生效，所以运行 `fallc` 前先开一个新终端（或在当前终端里也设一次
`FALLTHROUGH_STANDARD`）。把 `%LOCALAPPDATA%\Fallthrough\bin` 加到 `PATH`（系统属性 →
环境变量），`fallc` 就能直接按名字调用。

用 `--emit=static` 或 `--emit=shared` 产出的库在 Windows 上是 `.a` 与 `.dll`，链接它们需要
和编译编译器时相同的工具链。命令行选项在所有平台上都一样。

### Hello, world

写一个 `hello.ft`：

```ft
import C;

fn main() int {
    C.printf("Hello, world!\n");
    return 0;
}
```

编译并运行：

```sh
fallc hello.ft -o hello
./hello
```

用语言自己的标准库写同一个程序：

```ft
import std;

fn main() int except {
    std.println("Hello, world!")?!;
    return 0;
}
```

写流可能失败，所以 `println` 声明为 `except`，调用点要么处理异常，要么用 `?!` 把它传出去；
可能抛出的 `main` 要在返回类型后写明 `except`。

`import C;` 是用本语言写成的 C 标准库绑定；`import std;` 是语言自己的标准库。两者都是普通
模块，和程序自己的模块一样通过 `import` 使用。

默认可执行文件。其他产物用 `--emit=` 指定：

```sh
fallc hello.ft --emit=obj    -o hello.o     # 一个目标文件
fallc hello.ft --emit=static -o libhello.a  # 静态库
fallc hello.ft --emit=shared -o libhello.so # 共享库
fallc hello.ft --emit=llvm   -o hello.ll    # LLVM IR
```

多个 `.ft` 文件可以组成一个程序；`.ftm` 文件是供 `import` 的模块。

## 文档

语言的规范文本在 [docs/language](docs/language/README.md)：英文是主文档，每个分册都带中文
翻译（`*.zh.md`）。

| 文档 | 内容 |
| ---- | ---- |
| [00 总则](docs/language/00-start.zh.md) | 源文件、程序入口、编译选项、退出码、最初的诊断 |
| [语言规范](docs/language/README.zh.md) | 规范文本：第 00–10 分册与两个附录 |
| [01 词法](docs/language/01-lexical.zh.md) | 源文本、记号、注释、关键字、字面量、运算符 |
| [02 类型](docs/language/02-types.zh.md) | 类型系统、布局与对齐、隐式调整 |
| [03 声明](docs/language/03-declarations.zh.md) | 声明、属性、宏、traits、interface、模板 |
| [04 函数](docs/language/04-functions.zh.md) | 函数、成员函数、lambda、实参传递、重载、移动 |
| [05 语句](docs/language/05-statements.zh.md) | 块、循环、`switch`、跳转、`defer` |
| [06 异常](docs/language/06-exceptions.zh.md) | `raise`、`try`、`except`、`?!`、调用点义务 |
| [07 表达式](docs/language/07-expressions.zh.md) | 运算符、转换、特殊方法、求值顺序 |
| [08 模块](docs/language/08-modules.zh.md) | 导入与导出、模块查找、多文件程序 |
| [09 作用域](docs/language/09-scopes.zh.md) | 作用域、遮蔽、生命周期、销毁顺序 |
| [10 诊断](docs/language/10-diagnostics.zh.md) | 诊断编号、警告分组、抑制 |
| [附录 A：诊断](docs/language/appendix-a-errors.zh.md) | 每个稳定诊断编号、级别与消息模板 |
| [附录 B：特殊方法](docs/language/appendix-b-special-methods.zh.md) | `__init__`、`__drop__`、运算符、转换及其余 |
| [标准库](docs/library.zh.md) | `standard/std/` 下的模块、类型与函数 |
| [容器](docs/collections.zh.md) | `Array`、`String` 及其他容器的深入说明 |
| [模板](docs/templates.zh.md) | 参数包、折叠、`sizeof...` 以及需要示例说明的规则 |
| [C 绑定](docs/c-bindings.zh.md) | `C` 模块与 C ABI 调用的工作方式 |
| [SPEC-STATUS](docs/SPEC-STATUS.md) | 规范背后的决定，以及仍然开放的部分 |
| [示例](examples/) | 测试套件实际编译并运行的小程序 |

本文件的中文版是 [README.zh.md](README.zh.md)；英文主文档是 [README.md](README.md)。

## 仓库结构

```
crates/            编译器：span、诊断、词法、AST、语法、语义、IR、驱动
standard/          用 Fallthrough 编写的标准库与 C 绑定
examples/          小程序
docs/              规范、库参考与设计记录
```

编译器按阶段分 crate，crate 的职责就是阶段：`fallc-span` 与 `fallc-diag` 是基础，
`fallc-lexer` 把文本变成记号，`fallc-parser` 把记号变成 AST，`fallc-sema` 解析名字并检查
类型，`fallc-ir` 把检查后的树直接降到 LLVM IR，`fallc` 是唯一做 I/O 的 crate。

## 开发

仓库就是一个普通的 Cargo workspace，`make` 是常用任务的短命令：`make help` 列出全部，
`make release` 构建，`make install` 安装。`make check` 做编译期检查，`make clippy` 保持
无警告。

添加语言特性时按能让树始终可编译的顺序：

1. `crates/fallc-lexer/src/token.rs` 中的记号种类以及关键字/标点表；
2. `crates/fallc-ast/src/node/` 中的 AST 变体**和** `crates/fallc-ast/src/visit.rs` 中对应
   的分支；
3. `crates/fallc-parser/src/parser/` 中的语法产生式，以及 `precedence.rs` 中的运算符条目；
4. `crates/fallc-sema` 中的解析与类型规则；
5. `crates/fallc-ir/src/codegen/` 中的降级，以及 `crates/fallc-ir/src/types.rs` 中的类型
   映射——没有操作码枚举要扩展：直接发出 LLVM 指令，交给验证器检查结果。

一条可观察的规则，在测试钉住它、并且 `docs/language` 里两份语言（英文与中文）的规范段落都写好
之前，都还不算完成。
