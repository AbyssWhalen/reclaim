//! 并行扫描：遍历磁盘，找出所有命中规则的垃圾目录。
//!
//! 两个关键设计：
//! 1. **关闭 ignore 的所有默认过滤**（`standard_filters(false)`）。否则
//!    `node_modules`、`target`、`.venv` 这些目录几乎都被 .gitignore 命中或
//!    属于隐藏目录，会被 walker 直接跳过——那整个工具就废了。我们要找的
//!    恰恰是这些被忽略的东西。
//! 2. **命中即剪枝**（`WalkState::Skip`）。确认一个目录是垃圾后，没有理由
//!    再钻进它内部去看——既快又避免在 node_modules 里挖出一堆嵌套误报。

use crate::rules::{self, Rule};
use ignore::{WalkBuilder, WalkState};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// 一处垃圾发现。扫描阶段填充路径与命中的规则信息，
/// `size` 与 `newest_mtime_secs` 由 sizer 在后续阶段补齐。
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub path: PathBuf,
    pub ecosystem: &'static str,
    pub target: &'static str,
    pub note: &'static str,
    pub caution: bool,
    /// 目录总字节数，由 sizer 填充；扫描阶段为 0。
    pub size: u64,
    /// 目录内最新文件的修改时间（Unix 秒），由 sizer 填充。
    /// 用于 `--older-than` 陈旧度过滤。None 表示空目录或读取失败。
    pub newest_mtime_secs: Option<u64>,
}

impl Finding {
    fn from_rule(path: PathBuf, rule: &'static Rule) -> Self {
        Finding {
            path,
            ecosystem: rule.ecosystem,
            target: rule.target,
            note: rule.note,
            caution: rule.caution,
            size: 0,
            newest_mtime_secs: None,
        }
    }
}

/// 并行扫描给定的若干根目录，返回所有命中的垃圾目录。
///
/// 扫描本身不计算大小（那是 sizer 的活），所以返回的 Finding 的
/// `size` 字段此时均为 0。
pub fn scan(roots: &[PathBuf]) -> Vec<Finding> {
    if roots.is_empty() {
        return Vec::new();
    }

    let findings = Arc::new(Mutex::new(Vec::new()));

    let mut builder = WalkBuilder::new(&roots[0]);
    for r in &roots[1..] {
        builder.add(r);
    }
    // 一把关掉 gitignore / hidden / parents 等全部默认过滤。
    builder.standard_filters(false).follow_links(false);

    builder.build_parallel().run(|| {
        let findings = Arc::clone(&findings);
        Box::new(move |result| {
            let entry = match result {
                Ok(e) => e,
                Err(_) => return WalkState::Continue, // 权限不足等错误：跳过该项，不中断
            };

            // 只关心目录
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if !is_dir {
                return WalkState::Continue;
            }

            let name = match entry.file_name().to_str() {
                Some(n) => n,
                None => return WalkState::Continue, // 非 UTF-8 目录名：跳过
            };

            // 廉价预筛：绝大多数普通目录在此一步被刷掉，不触发 read_dir
            if !rules::is_candidate(name) {
                return WalkState::Continue;
            }

            // 命中候选名，读取兄弟文件做精确判定
            let path = entry.path();
            let siblings = read_siblings(path);
            if let Some(rule) = rules::match_junk(name, &siblings) {
                findings
                    .lock()
                    .unwrap()
                    .push(Finding::from_rule(path.to_path_buf(), rule));
                // 已确认是垃圾：不再钻进去
                return WalkState::Skip;
            }

            WalkState::Continue
        })
    });

    Arc::try_unwrap(findings)
        .expect("扫描结束后不应再有其他 Arc 引用")
        .into_inner()
        .unwrap()
}

/// 读取 `dir` 所在父目录中的所有条目名（即 dir 的兄弟）。
/// 标记文件（Cargo.toml、pom.xml…）和目标目录是同级关系，
/// 所以判定时要看的是父目录的内容。
fn read_siblings(dir: &Path) -> Vec<String> {
    let parent = match dir.parent() {
        Some(p) => p,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(parent) {
        for entry in rd.flatten() {
            if let Some(n) = entry.file_name().to_str() {
                out.push(n.to_string());
            }
        }
    }
    out
}
