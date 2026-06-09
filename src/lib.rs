//! reclaim —— 上下文感知的项目垃圾清理器。
//!
//! 库入口，对外暴露各模块，供二进制（`main.rs`）和集成测试（`tests/`）共用。
//! 二进制只负责 CLI 解析与流程编排，核心逻辑全在这些模块里且各自带单测。
//!
//! 数据流：
//! ```text
//! scan(roots)  →  Vec<Finding>（仅路径+规则，size=0）
//!     │
//! sizer::measure_all  →  填充 size 与 newest_mtime_secs
//!     │
//! filter::filter_stale  →  按 --older-than 滤掉「最近还在用」的
//!     │
//!     ├── report::to_json   →  --json 输出给脚本
//!     └── tui::run          →  交互勾选 → deleter::delete_one
//! ```

pub mod deleter;
pub mod filter;
pub mod report;
pub mod rules;
pub mod scanner;
pub mod sizer;
pub mod tui;

/// 安全闸门：是否应拦截「无人值守的永久删除」。
///
/// `--force`(永久删) + `--yes`(跳过交互) 同时出现，意味着「永久删除一切、
/// 零人工确认」——最危险的组合。要求再显式加 `--really` 才放行，防止用户
/// 从 README / 聊天里整条复制 `--force --yes` 时手滑清空磁盘。
///
/// `--dry-run` 不删任何东西，所以预演永不拦截。
///
/// 抽成纯函数是为了能脱离 CLI 单测——main.rs 的判断直接调用它。
pub fn should_block_unattended_purge(force: bool, yes: bool, really: bool, dry_run: bool) -> bool {
    force && yes && !really && !dry_run
}

#[cfg(test)]
mod gate_tests {
    use super::should_block_unattended_purge;

    #[test]
    fn blocks_force_yes_without_really() {
        // 最危险组合，无 --really：必须拦
        assert!(should_block_unattended_purge(true, true, false, false));
    }

    #[test]
    fn really_lets_it_through() {
        // 显式 --really：放行
        assert!(!should_block_unattended_purge(true, true, true, false));
    }

    #[test]
    fn dry_run_never_blocked() {
        // 预演不删东西，即便 force+yes 也不拦
        assert!(!should_block_unattended_purge(true, true, false, true));
    }

    #[test]
    fn safe_combos_pass() {
        // 只 --force（进 TUI 手动勾）、只 --yes（走回收站可恢复）都不是危险组合
        assert!(!should_block_unattended_purge(true, false, false, false));
        assert!(!should_block_unattended_purge(false, true, false, false));
        assert!(!should_block_unattended_purge(false, false, false, false));
    }
}
