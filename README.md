<div align="center">

# 🧹 reclaim

**上下文感知的项目垃圾清理器 · Context-aware project junk cleaner**

扫出散落各处的 `node_modules` / `target` / `__pycache__` / `.venv` 等构建产物与缓存，按占用大小排序，交互勾选后安全移入回收站。

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange.svg?logo=rust)](https://www.rust-lang.org)
[![CI](https://github.com/AbyssWhalen/reclaim/actions/workflows/ci.yml/badge.svg)](https://github.com/AbyssWhalen/reclaim/actions/workflows/ci.yml)
[![Platform](https://img.shields.io/badge/platform-Windows-blue.svg)](#-平台支持)
[![Tests](https://img.shields.io/badge/tests-37%20passing-brightgreen.svg)](#-开发与测试)

</div>

---

## 为什么不是「又一个清理器」

大多数清理工具靠**目录名**判断：看见 `target/` 就删。可 `target/` 也可能是你亲手建的资源目录、`build/` 可能是网站源码——一刀切就会误删。

**reclaim 靠上下文判断**：`target/` 只有在**旁边有 `Cargo.toml`** 时才认定为 Rust 构建产物；`build/` 必须有 `build.gradle` 才动；`bin/` 要旁边有 `*.csproj` 才算 .NET 输出。判定哲学一句话——

> **宁可漏删，不可误删。**

实测对比（沙箱真实输出，见下文）：同一棵目录树里，带 `Cargo.toml` 的 `target/` 被清理，而手建的、没有 `Cargo.toml` 的 `target/` 被**完整保留**。

## ✨ 特性

| | |
|---|---|
| 🎯 **上下文识别** | 标记文件门控，不靠裸目录名，从根上防误删 |
| ⏳ **陈旧度过滤** | `--older-than 30d` 只清吃灰的项目，正在开发的不动 |
| 🗑️ **默认可恢复** | 默认移入系统回收站，`--force` 才永久删除 |
| 🛡️ **纵深防御** | 删除前四道安全关卡，系统目录/盘符根一律拒绝 |
| ⚡ **并行扫描** | 基于 `ignore` + `rayon`，多线程遍历与算大小 |
| 🖥️ **交互 TUI** | 按大小排序，空格勾选，一眼看清谁在占地 |
| 🤖 **脚本友好** | `--json` 输出结构化结果，接管道、进 CI |
| 🌐 **多生态** | Node / Rust / Python / Gradle / Maven / .NET / CMake / Xcode / Unity / Go |

## 📺 演示

> 交互界面（TUI）实景——按占用从大到小排列，空格勾选，`d` 删除：

```text
 reclaim — 共 4 项垃圾，合计 17 MB
┌──────────────────────────────────────────────────────────────────────────┐
│› [ ]       8 MB  Python    …\reclaim_demo\my-py\.venv                       │
│  [ ]       5 MB  Node      …\reclaim_demo\my-web\node_modules               │
│  [ ]       3 MB  Rust      …\reclaim_demo\my-rust-app\target                │
│  [ ]       1 MB  Python    …\reclaim_demo\my-py\__pycache__                 │
└──────────────────────────────────────────────────────────────────────────┘
 ↑/↓ 移动   空格 勾选   a 全选   d 删除   q 退出    │    已选 0 项 / 0 B
```

<!-- TODO: 可用 asciinema 录一段真实操作替换上面的静态示意：
     asciinema rec demo.cast  然后传到 asciinema.org 贴链接，或转成 gif。 -->

## 📦 安装

目前从源码构建（暂未发布到 crates.io）。需要 [Rust 工具链](https://rustup.rs)（1.85+，因使用 2024 edition）。

```bash
git clone https://github.com/AbyssWhalen/reclaim.git
cd reclaim
cargo build --release
# 产物在 target/release/reclaim(.exe)
```

可选：把二进制加入 PATH，或 `cargo install --path .` 装到本地 cargo bin。

## 🚀 用法

```bash
# 扫描当前目录，进入交互界面勾选删除
reclaim

# 扫描指定目录（可多个）
reclaim D:\code C:\Users\me\projects

# 只清 30 天没动过的项目产物（正在开发的不受影响）
reclaim --older-than 30d D:\code

# 预演：看看会删什么，但不真删（配合 --yes 跳过交互、全量预览）
reclaim --yes --dry-run D:\code

# 输出 JSON 给脚本 / 管道消费
reclaim --json D:\code

# 永久删除而非进回收站（不可恢复，慎用）
reclaim --force D:\code
```

### 预演输出示例（真实运行）

```text
$ reclaim --yes --dry-run
正在扫描 1 个根目录…
发现 4 项，正在计算大小…
将删除（预演）        1 MB  …\my-py\__pycache__
将删除（预演）        5 MB  …\my-web\node_modules
将删除（预演）        3 MB  …\my-rust-app\target
将删除（预演）        8 MB  …\my-py\.venv

完成：4 项，预计可释放 17 MB。（预演模式，未实际删除）
```

### JSON 输出示例（真实运行，节选）

```json
{
  "summary": { "total_count": 4, "total_bytes": 17000000 },
  "by_ecosystem": {
    "Node":   { "count": 1, "bytes": 5000000 },
    "Python": { "count": 2, "bytes": 9000000 },
    "Rust":   { "count": 1, "bytes": 3000000 }
  },
  "findings": [
    {
      "path": ".\\my-rust-app\\target",
      "ecosystem": "Rust",
      "target": "target",
      "note": "Cargo 构建产物，可由 cargo build 重建",
      "caution": false,
      "size": 3000000,
      "newest_mtime_secs": 1780977349
    }
  ]
}
```

### 全部选项

```text
Usage: reclaim [OPTIONS] [PATH]...

Arguments:
  [PATH]...  要扫描的根目录（可多个）。省略时扫描当前目录。

Options:
      --older-than <DURATION>  只清理「最近一次改动距今 ≥ 该时长」的目录。
                               语法：数字 + 单位 s/m/h/d/w，例如 30d、2w。
      --json                   输出 JSON 给脚本消费，不进入交互界面。
      --dry-run                预演模式：只显示将要删除什么，不真正删除。
      --force                  永久删除而非移入回收站（不可恢复，慎用）。
      --yes                    跳过 TUI，直接处理所有扫描到的项。仍受安全校验保护。
  -h, --help                   Print help
  -V, --version                Print version
```

## 🛡️ 安全设计

删除是不可逆的高危操作，reclaim 在真正删任何东西前，每个路径都要通过**四道纯函数关卡**（全部有单元测试覆盖）：

1. **必须绝对路径** — 来源不明的相对路径一律拒绝。
2. **basename 必须是已知垃圾名** — 即便上游逻辑有 bug 把 `src` 这种源码目录传进来，这关也拦下。能删到的，只可能是垃圾目录名。
3. **深度下限** — 拒绝盘符根的直接子目录（如 `C:\Windows`），垃圾产物正常都嵌在数层之下。
4. **系统黑名单 + 用户主目录** — 大小写不敏感地拒绝 `Windows`、`Program Files`、`System32` 等关键目录，以及用户主目录本身。

再加两层兜底：默认走**回收站**（可恢复），`--dry-run` 可随时**预演**。

## 🧩 支持的生态

| 生态 | 识别目标 | 判定条件 |
|---|---|---|
| Node | `node_modules` | 目录名即可 |
| Node | `.next` `.nuxt` `.turbo` | 旁有对应配置文件 |
| Rust | `target` | 旁有 `Cargo.toml` |
| Python | `__pycache__` `.venv` `.pytest_cache` `.mypy_cache` `.ruff_cache` | 目录名即可 |
| Gradle | `.gradle` `build` | 旁有 `build.gradle` 等 |
| Maven | `target` | 旁有 `pom.xml` |
| .NET | `bin` `obj` | 旁有 `*.csproj/.fsproj/.vbproj` |
| CMake | `cmake-build-*` | 旁有 `CMakeLists.txt` |
| Xcode | `DerivedData` `Pods` | 目录名 / 旁有 `Podfile` |
| Unity | `Library` ⚠️ | 旁有 `ProjectSettings`，重建较慢 |
| Go | `vendor` ⚠️ | 旁有 `go.mod`，离线构建依赖它 |

> ⚠️ 标记的项删除代价较高，TUI 中会黄色高亮提示。新增生态只需在 `src/rules.rs` 的规则表里加一行。

## 🖥️ 平台支持

诚实说明，不夸大：

| 平台 | 状态 |
|---|---|
| **Windows** | ✅ 已开发并测试（37 项测试通过，含真实删除验证） |
| Linux / macOS | ⚠️ 应可编译，但**未经测试**；且当前安全黑名单偏 Windows（`System32` 等），跨平台前需补充 Unix 系统目录规则与路径用例 |

跨平台适配已列入 Roadmap，欢迎 PR。

## 🧪 开发与测试

```bash
cargo test            # 运行全部测试（37 项：单元 + 集成）
cargo clippy          # lint
cargo fmt             # 格式化
```

测试结构：
- **单元测试** — 规则匹配、安全校验、陈旧度解析、TUI 选择逻辑，全是不碰文件系统的纯函数。
- **集成测试**（`tests/integration.rs`）— 在系统临时目录造一棵假项目树，端到端验证「只命中垃圾、放过源码」，结束自动清理。

## 🗺️ Roadmap

- [ ] 全局包管理器缓存清理（`~/.cargo`、`.npm`、`.gradle` 等）
- [ ] 配置文件自定义规则（用户私有生态）
- [ ] git 感知：对未提交改动的目录额外警告
- [ ] 跨平台：Unix 系统目录黑名单 + Linux/macOS 测试矩阵
- [ ] 发布到 crates.io

## 🤝 贡献

欢迎 issue 与 PR。新增清理规则尤其简单——见 `src/rules.rs` 顶部说明，加一行规则、配一个单测即可。

## 📄 License

[MIT](./LICENSE) © AbyssWhalen
