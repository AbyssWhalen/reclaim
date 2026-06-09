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
