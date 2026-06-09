//! JSON 输出：把扫描结果转成结构化 JSON，供脚本/管道消费。
//!
//! 设计：人看的归 TUI，机器读的归这里。输出包含三部分——
//! - `findings`：逐条垃圾目录（路径、生态、大小、最新 mtime）
//! - `summary`：总条数、总字节数
//! - `by_ecosystem`：按生态分组的条数与字节数，方便一眼看清哪类最占地

use crate::scanner::Finding;
use serde::Serialize;
use std::collections::BTreeMap;

/// 单条生态汇总。
#[derive(Debug, Serialize)]
struct EcoSummary {
    count: usize,
    bytes: u64,
}

/// 整体汇总。
#[derive(Debug, Serialize)]
struct Summary {
    total_count: usize,
    total_bytes: u64,
}

/// 完整 JSON 报告的顶层结构。
#[derive(Debug, Serialize)]
struct Report<'a> {
    summary: Summary,
    by_ecosystem: BTreeMap<&'static str, EcoSummary>,
    findings: &'a [Finding],
}

/// 把 findings 序列化为格式化 JSON 字符串。
pub fn to_json(findings: &[Finding]) -> String {
    let total_bytes: u64 = findings.iter().map(|f| f.size).sum();

    let mut by_ecosystem: BTreeMap<&'static str, EcoSummary> = BTreeMap::new();
    for f in findings {
        let entry = by_ecosystem
            .entry(f.ecosystem)
            .or_insert(EcoSummary { count: 0, bytes: 0 });
        entry.count += 1;
        entry.bytes = entry.bytes.saturating_add(f.size);
    }

    let report = Report {
        summary: Summary {
            total_count: findings.len(),
            total_bytes,
        },
        by_ecosystem,
        findings,
    };

    // 序列化不应失败（结构全部可序列化）；万一失败给个合法的空对象兜底。
    serde_json::to_string_pretty(&report).unwrap_or_else(|_| String::from("{}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn mk(ecosystem: &'static str, size: u64) -> Finding {
        Finding {
            path: PathBuf::from(format!("/tmp/{ecosystem}/junk")),
            ecosystem,
            target: "x",
            note: "n",
            caution: false,
            size,
            newest_mtime_secs: Some(1_700_000_000),
        }
    }

    #[test]
    fn empty_is_valid_json() {
        let out = to_json(&[]);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["summary"]["total_count"], 0);
        assert_eq!(v["summary"]["total_bytes"], 0);
    }

    #[test]
    fn totals_and_grouping() {
        let findings = vec![mk("Rust", 100), mk("Node", 50), mk("Rust", 25)];
        let out = to_json(&findings);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();

        assert_eq!(v["summary"]["total_count"], 3);
        assert_eq!(v["summary"]["total_bytes"], 175);
        // Rust 两条共 125
        assert_eq!(v["by_ecosystem"]["Rust"]["count"], 2);
        assert_eq!(v["by_ecosystem"]["Rust"]["bytes"], 125);
        assert_eq!(v["by_ecosystem"]["Node"]["count"], 1);
        assert_eq!(v["by_ecosystem"]["Node"]["bytes"], 50);
    }
}
