//! 算大小：对扫描得到的每个垃圾目录，并行统计其总字节数，
//! 并顺手记录目录内最新文件的修改时间（给陈旧度过滤复用）。
//!
//! 设计要点：
//! - 各 Finding 之间用 rayon 并行（典型机器上有几十到几百个命中目录）。
//! - 单个目录内部用普通递归 walkdir，不再嵌套 rayon——避免线程爆炸，
//!   且大目录的成本已经被目录间并行摊开。
//! - 遍历每个文件时同时累加大小、刷新最新 mtime，一趟搞定，零额外开销。

use crate::scanner::Finding;
use rayon::prelude::*;
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

/// 为所有 Finding 并行填充 `size` 与 `newest_mtime_secs`。
pub fn measure_all(findings: &mut [Finding]) {
    findings.par_iter_mut().for_each(|f| {
        let (size, newest) = measure_dir(&f.path);
        f.size = size;
        f.newest_mtime_secs = newest;
    });
}

/// 统计单个目录的总字节数与最新修改时间。
///
/// 返回 `(总字节数, 最新 mtime 秒)`。最新 mtime 为 None 表示
/// 目录为空或所有文件的时间都读不出来。
fn measure_dir(root: &std::path::Path) -> (u64, Option<u64>) {
    let mut total: u64 = 0;
    let mut newest: Option<u64> = None;

    for entry in WalkDir::new(root).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue, // 权限不足等错误：跳过该项
        };
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_file() {
            total = total.saturating_add(meta.len());
        }
        // 文件和目录的 mtime 都纳入考虑：取最新的一个
        if let Ok(modified) = meta.modified()
            && let Ok(dur) = modified.duration_since(UNIX_EPOCH)
        {
            let secs = dur.as_secs();
            newest = Some(newest.map_or(secs, |cur| cur.max(secs)));
        }
    }

    (total, newest)
}

/// 当前时间的 Unix 秒。供 `--older-than` 过滤计算「距今多久」。
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
