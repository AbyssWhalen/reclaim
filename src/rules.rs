//! 垃圾规则表 —— 整个项目的核心资产。
//!
//! 判定哲学：宁可漏删，不可误删。
//!
//! 规则分两类：
//! - **自证型**（`markers` 为空）：目录名本身就足以确认是构建/缓存产物，
//!   例如 `node_modules`、`__pycache__`。光看名字就能删。
//! - **标记门控型**（`markers` 非空）：目录名是通用词，可能是源码目录
//!   （`target`、`build`、`bin`），必须在它的**兄弟文件**里找到标记文件
//!   （如 `Cargo.toml`），才认定为垃圾。否则放过。
//!
//! 加新生态只改这张表，匹配逻辑不动。

use serde::Serialize;

/// 一条垃圾识别规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Rule {
    /// 生态名，用于展示与分组，例如 "Rust"、"Node"。
    pub ecosystem: &'static str,
    /// 要匹配的目录名，例如 "target"、"node_modules"。
    pub target: &'static str,
    /// 标记文件列表。为空表示自证型，光凭目录名即可判定；
    /// 非空表示需要在该目录的兄弟文件中命中任意一个才算垃圾。
    pub markers: &'static [&'static str],
    /// 是否需要谨慎对待。`vendor`、`Library` 这类删了可能要重新拉取/
    /// 重新生成且代价较高的目录置为 true，TUI 中会高亮提示。
    pub caution: bool,
    /// 一句话说明这是什么，展示给用户看。
    pub note: &'static str,
}

/// 内置规则表。顺序无关紧要，匹配时全表线性扫描（规则数量很小）。
pub static RULES: &[Rule] = &[
    // ---- Node / 前端 ----
    Rule {
        ecosystem: "Node",
        target: "node_modules",
        markers: &[],
        caution: false,
        note: "npm/yarn/pnpm 依赖，可由 lockfile 重装",
    },
    Rule {
        ecosystem: "Node",
        target: ".next",
        markers: &["next.config.js", "next.config.mjs", "next.config.ts"],
        caution: false,
        note: "Next.js 构建缓存",
    },
    Rule {
        ecosystem: "Node",
        target: ".turbo",
        markers: &["turbo.json"],
        caution: false,
        note: "Turborepo 缓存",
    },
    Rule {
        ecosystem: "Node",
        target: ".nuxt",
        markers: &["nuxt.config.js", "nuxt.config.ts"],
        caution: false,
        note: "Nuxt 构建产物",
    },
    // ---- Rust ----
    Rule {
        ecosystem: "Rust",
        target: "target",
        markers: &["Cargo.toml"],
        caution: false,
        note: "Cargo 构建产物，可由 cargo build 重建",
    },
    // ---- Python ----
    Rule {
        ecosystem: "Python",
        target: "__pycache__",
        markers: &[],
        caution: false,
        note: "Python 字节码缓存",
    },
    Rule {
        ecosystem: "Python",
        target: ".venv",
        markers: &[],
        caution: false,
        note: "虚拟环境，可由 requirements/pyproject 重建",
    },
    Rule {
        ecosystem: "Python",
        target: ".pytest_cache",
        markers: &[],
        caution: false,
        note: "pytest 缓存",
    },
    Rule {
        ecosystem: "Python",
        target: ".mypy_cache",
        markers: &[],
        caution: false,
        note: "mypy 类型检查缓存",
    },
    Rule {
        ecosystem: "Python",
        target: ".ruff_cache",
        markers: &[],
        caution: false,
        note: "ruff 检查缓存",
    },
    // ---- Java / Gradle / Maven ----
    Rule {
        ecosystem: "Gradle",
        target: ".gradle",
        markers: &[
            "build.gradle",
            "build.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts",
        ],
        caution: false,
        note: "Gradle 本地缓存",
    },
    Rule {
        ecosystem: "Gradle",
        target: "build",
        markers: &["build.gradle", "build.gradle.kts"],
        caution: false,
        note: "Gradle 构建输出",
    },
    Rule {
        ecosystem: "Maven",
        target: "target",
        markers: &["pom.xml"],
        caution: false,
        note: "Maven 构建输出",
    },
    // 注意：.NET 的 bin/obj 不在此表中。它们的目录名是通用词，
    // 判定完全交给下方 SUFFIX_MARKERS（必须旁边有 *.csproj/*.fsproj/*.vbproj），
    // 以免裸 bin/obj 目录被误判为垃圾。
    // ---- C/C++ CMake ----
    Rule {
        ecosystem: "CMake",
        target: "cmake-build-debug",
        markers: &["CMakeLists.txt"],
        caution: false,
        note: "CMake Debug 构建目录",
    },
    Rule {
        ecosystem: "CMake",
        target: "cmake-build-release",
        markers: &["CMakeLists.txt"],
        caution: false,
        note: "CMake Release 构建目录",
    },
    // ---- Apple / Xcode ----
    Rule {
        ecosystem: "Xcode",
        target: "DerivedData",
        markers: &[],
        caution: false,
        note: "Xcode 派生数据",
    },
    Rule {
        ecosystem: "CocoaPods",
        target: "Pods",
        markers: &["Podfile"],
        caution: false,
        note: "CocoaPods 依赖",
    },
    // ---- Unity ----
    Rule {
        ecosystem: "Unity",
        target: "Library",
        markers: &["Assembly-CSharp.csproj", "ProjectSettings"],
        caution: true,
        note: "Unity 导入缓存，重建耗时较长",
    },
    // ---- Go（谨慎：vendor 在离线构建时是必需的）----
    Rule {
        ecosystem: "Go",
        target: "vendor",
        markers: &["go.mod"],
        caution: true,
        note: "Go vendored 依赖，离线构建依赖它",
    },
];

/// .NET 的 bin/obj 是后缀型命中时返回的规则实例。
/// 单独定义成静态，供 SUFFIX_MARKERS 直接引用——这样新增后缀标记时
/// 编译器强制要求就地给出对应规则，杜绝「忘记更新分支」导致的运行时 panic。
static DOTNET_BIN: Rule = Rule {
    ecosystem: ".NET",
    target: "bin",
    markers: &[],
    caution: false,
    note: ".NET 编译输出",
};
static DOTNET_OBJ: Rule = Rule {
    ecosystem: ".NET",
    target: "obj",
    markers: &[],
    caution: false,
    note: ".NET 中间产物",
};

/// 后缀型标记：某些 target 目录（如 .NET 的 bin/obj）的判定不依赖固定文件名，
/// 而是兄弟文件中存在某个**扩展名**。这里列出 (target, 后缀, 命中时返回的规则) 三元组。
/// 每条都直接携带规则引用，命中即返回，无需二次查表。
static SUFFIX_MARKERS: &[(&str, &str, &Rule)] = &[
    ("bin", ".csproj", &DOTNET_BIN),
    ("bin", ".fsproj", &DOTNET_BIN),
    ("bin", ".vbproj", &DOTNET_BIN),
    ("obj", ".csproj", &DOTNET_OBJ),
    ("obj", ".fsproj", &DOTNET_OBJ),
    ("obj", ".vbproj", &DOTNET_OBJ),
];

/// 快速预筛：目录名是否可能命中某条规则（含后缀型）。
///
/// 扫描器在读取兄弟文件之前用它廉价过滤——绝大多数普通目录（`src`、`docs`…）
/// 一次字符串比较就被刷掉，根本不会触发额外的 read_dir。
pub fn is_candidate(dir_name: &str) -> bool {
    RULES.iter().any(|r| r.target == dir_name)
        || SUFFIX_MARKERS.iter().any(|&(t, _, _)| t == dir_name)
}

/// 核心匹配：纯函数，不碰文件系统，便于单测。
///
/// 给定一个目录名 `dir_name` 和它所在父目录中的兄弟条目名 `siblings`，
/// 返回命中的规则（若有）。
///
/// 匹配优先级：
/// 1. 自证型规则（markers 为空）——只比目录名。
/// 2. 标记门控规则——目录名匹配且兄弟中含任一标记文件。
/// 3. 后缀型标记——目录名匹配且兄弟中有以指定后缀结尾的文件。
pub fn match_junk(dir_name: &str, siblings: &[String]) -> Option<&'static Rule> {
    // 先扫固定标记规则
    for rule in RULES {
        if rule.target != dir_name {
            continue;
        }
        if rule.markers.is_empty() {
            // 自证型：目录名即可确认
            return Some(rule);
        }
        // 标记门控：兄弟中含任一标记文件
        if rule.markers.iter().any(|m| siblings.iter().any(|s| s == m)) {
            return Some(rule);
        }
    }

    // 再看后缀型标记（如 .NET bin/obj 旁边有 *.csproj）
    for &(target, suffix, rule) in SUFFIX_MARKERS {
        if target == dir_name && siblings.iter().any(|s| s.ends_with(suffix)) {
            return Some(rule);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn self_identifying_matches_by_name_only() {
        // node_modules 无需任何兄弟文件
        let hit = match_junk("node_modules", &[]).unwrap();
        assert_eq!(hit.ecosystem, "Node");
        assert_eq!(hit.target, "node_modules");
    }

    #[test]
    fn pycache_is_self_identifying() {
        assert!(match_junk("__pycache__", &[]).is_some());
        assert!(match_junk(".mypy_cache", &[]).is_some());
    }

    #[test]
    fn rust_target_requires_cargo_toml() {
        // 旁边有 Cargo.toml → 命中
        let hit = match_junk("target", &s(&["Cargo.toml", "src"])).unwrap();
        assert_eq!(hit.ecosystem, "Rust");
        // 旁边没有任何标记 → 放过（可能是用户自建的 target 源码目录）
        assert!(match_junk("target", &s(&["src", "main.c"])).is_none());
    }

    #[test]
    fn maven_target_requires_pom() {
        let hit = match_junk("target", &s(&["pom.xml"])).unwrap();
        assert_eq!(hit.ecosystem, "Maven");
    }

    #[test]
    fn dotnet_bin_requires_csproj_suffix() {
        let hit = match_junk("bin", &s(&["App.csproj", "Program.cs"])).unwrap();
        assert_eq!(hit.ecosystem, ".NET");
        // 没有 .csproj 的 bin 目录不动（可能是放可执行文件的普通目录）
        assert!(match_junk("bin", &s(&["script.sh"])).is_none());
    }

    #[test]
    fn gradle_build_requires_marker() {
        assert!(match_junk("build", &s(&["build.gradle"])).is_some());
        // 裸 build 目录（无 gradle 标记）放过
        assert!(match_junk("build", &s(&["index.html"])).is_none());
    }

    #[test]
    fn caution_flag_on_vendor_and_unity() {
        let go = match_junk("vendor", &s(&["go.mod"])).unwrap();
        assert!(go.caution);
        let unity = match_junk("Library", &s(&["ProjectSettings"])).unwrap();
        assert!(unity.caution);
    }

    #[test]
    fn unknown_dir_never_matches() {
        assert!(match_junk("src", &s(&["main.rs"])).is_none());
        assert!(match_junk("my_data", &[]).is_none());
    }
}
