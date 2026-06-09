//! 陈旧度过滤：`--older-than` 的解析与判定。
//!
//! 核心思路：扫描+算大小时已经记录了每个目录的「最新文件 mtime」
//! （`Finding::newest_mtime_secs`）。陈旧度过滤就是只保留那些
//! 「最近一次改动距今 ≥ 阈值」的目录——正在开发的项目刚动过，会被滤掉，
//! 只有吃灰的项目产物留下来。这才是「别动我在用的，清我不用的」。
//!
//! 时长语法：纯数字 + 单位后缀，单位取 `s/m/h/d/w` 之一。
//! 例：`30d` = 30 天，`2w` = 2 周，`12h` = 12 小时。无后缀视为非法。

use crate::scanner::Finding;

/// 把 `"30d"`、`"2w"` 这类时长字符串解析为秒数。
///
/// 合法格式：一段非负整数 + 单一单位后缀 `s|m|h|d|w`。
/// 解析失败返回 `Err`，附带人类可读的原因。
pub fn parse_duration(input: &str) -> Result<u64, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("时长为空".to_string());
    }

    // 拆出末尾的单位字符与前面的数字部分
    let unit = s.chars().last().unwrap();
    let (num_part, secs_per_unit) = match unit {
        's' => (&s[..s.len() - 1], 1u64),
        'm' => (&s[..s.len() - 1], 60),
        'h' => (&s[..s.len() - 1], 60 * 60),
        'd' => (&s[..s.len() - 1], 60 * 60 * 24),
        'w' => (&s[..s.len() - 1], 60 * 60 * 24 * 7),
        other => {
            return Err(format!(
                "无法识别的时长单位 `{other}`，应为 s/m/h/d/w 之一（如 30d、2w）"
            ));
        }
    };

    if num_part.is_empty() {
        return Err(format!("`{input}` 缺少数字部分"));
    }

    let value: u64 = num_part
        .parse()
        .map_err(|_| format!("`{num_part}` 不是合法的非负整数"))?;

    value
        .checked_mul(secs_per_unit)
        .ok_or_else(|| format!("`{input}` 时长溢出"))
}

/// 判定单个 Finding 是否「足够陈旧」：最新改动距今 ≥ `threshold_secs`。
///
/// `now_secs` 为当前 Unix 秒。若目录的 `newest_mtime_secs` 为 None
/// （空目录或时间读取失败），保守地**视为陈旧**（保留它）——
/// 因为读不到时间不该成为「跳过清理」的理由，而漏清比误判更可接受。
pub fn is_stale(f: &Finding, now_secs: u64, threshold_secs: u64) -> bool {
    match f.newest_mtime_secs {
        Some(mtime) => now_secs.saturating_sub(mtime) >= threshold_secs,
        None => true,
    }
}

/// 按陈旧度过滤：仅保留距今改动 ≥ 阈值的 Finding。
/// `threshold_secs` 为 0 时不过滤（全部保留）。
pub fn filter_stale(findings: Vec<Finding>, now_secs: u64, threshold_secs: u64) -> Vec<Finding> {
    if threshold_secs == 0 {
        return findings;
    }
    findings
        .into_iter()
        .filter(|f| is_stale(f, now_secs, threshold_secs))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn mk(newest_mtime_secs: Option<u64>) -> Finding {
        Finding {
            path: PathBuf::from("/tmp/x/node_modules"),
            ecosystem: "Node",
            target: "node_modules",
            note: "n",
            caution: false,
            size: 1,
            newest_mtime_secs,
        }
    }

    #[test]
    fn parses_common_units() {
        assert_eq!(parse_duration("30s").unwrap(), 30);
        assert_eq!(parse_duration("5m").unwrap(), 300);
        assert_eq!(parse_duration("2h").unwrap(), 7200);
        assert_eq!(parse_duration("1d").unwrap(), 86_400);
        assert_eq!(parse_duration("2w").unwrap(), 1_209_600);
    }

    #[test]
    fn trims_whitespace() {
        assert_eq!(parse_duration("  7d  ").unwrap(), 604_800);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("30").is_err()); // 无单位
        assert!(parse_duration("d").is_err()); // 无数字
        assert!(parse_duration("30x").is_err()); // 非法单位
        assert!(parse_duration("-5d").is_err()); // 负数（parse u64 失败）
        assert!(parse_duration("abcd").is_err());
    }

    #[test]
    fn stale_when_old_enough() {
        let now = 1_000_000u64;
        // 改动于 now - 100，阈值 50 → 陈旧
        let f = mk(Some(now - 100));
        assert!(is_stale(&f, now, 50));
        // 改动于 now - 10，阈值 50 → 不够旧
        let recent = mk(Some(now - 10));
        assert!(!is_stale(&recent, now, 50));
    }

    #[test]
    fn missing_mtime_treated_as_stale() {
        let f = mk(None);
        assert!(is_stale(&f, 1_000_000, 999_999));
    }

    #[test]
    fn zero_threshold_keeps_all() {
        let now = 1_000_000u64;
        let findings = vec![mk(Some(now - 1)), mk(Some(now - 999_999))];
        let kept = filter_stale(findings, now, 0);
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn filter_keeps_only_stale() {
        let now = 1_000_000u64;
        let findings = vec![
            mk(Some(now - 10)),      // 新，应滤掉
            mk(Some(now - 100_000)), // 旧，应保留
        ];
        let kept = filter_stale(findings, now, 50_000);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].newest_mtime_secs, Some(now - 100_000));
    }
}
