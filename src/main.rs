//! reclaim CLI —— 薄编排层。
//!
//! 只做两件事：解析命令行参数、把 lib 里的各阶段串起来。
//! 所有核心逻辑（规则、扫描、算大小、过滤、安全删除）都在 lib 模块里，
//! 各自带单测；这里不重复实现任何业务逻辑。

use clap::Parser;
use humansize::{DECIMAL, format_size};
use reclaim::deleter::{self, DeleteMode};
use reclaim::filter;
use reclaim::report;
use reclaim::scanner;
use reclaim::sizer;
use reclaim::tui::{self, Outcome};
use std::path::PathBuf;
use std::process::ExitCode;

/// 上下文感知的项目垃圾清理器：扫出 node_modules / target / __pycache__
/// 等构建产物与缓存，按大小排序，交互勾选后移入回收站。
/// Context-aware project junk cleaner.
#[derive(Parser, Debug)]
#[command(name = "reclaim", version, about, long_about = None)]
struct Cli {
    /// 要扫描的根目录（可多个）。省略时扫描当前目录。
    /// Root directories to scan (defaults to current dir).
    #[arg(value_name = "PATH")]
    paths: Vec<PathBuf>,

    /// 只清理「最近一次改动距今 ≥ 该时长」的目录。语法：数字 + 单位 s/m/h/d/w，例如 30d、2w。
    /// Only clean dirs untouched for at least this long (e.g. 30d, 2w).
    #[arg(long, value_name = "DURATION")]
    older_than: Option<String>,

    /// 输出 JSON 给脚本消费，不进入交互界面。
    /// Emit JSON for scripts instead of the interactive UI.
    #[arg(long)]
    json: bool,

    /// 预演模式：只显示将要删除什么，不真正删除。
    /// Dry run: show what would be deleted without deleting.
    #[arg(long)]
    dry_run: bool,

    /// 永久删除而非移入回收站（不可恢复，慎用）。
    /// Permanently delete instead of moving to trash (irreversible).
    #[arg(long)]
    force: bool,

    /// 跳过 TUI，直接处理所有扫描到的项。仍受安全校验保护。
    /// Skip the TUI and process every item found (still safety-checked).
    #[arg(long)]
    yes: bool,

    /// 确认无人值守的永久删除。仅在 --force 与 --yes 同时使用时需要。
    /// Required to confirm unattended permanent deletion (with --force --yes).
    #[arg(long)]
    really: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // 安全闸门：无人值守的永久删除（--force --yes）必须再加 --really 才放行。
    // 防止用户从 README/聊天里整条复制 `--force --yes` 时手滑清空磁盘。
    // Safety gate: unattended permanent deletion requires an explicit --really.
    if reclaim::should_block_unattended_purge(cli.force, cli.yes, cli.really, cli.dry_run) {
        eprintln!(
            "拒绝执行：--force --yes 会无确认地永久删除所有扫描结果。\n\
             如果你确定，请追加 --really；或先用 --dry-run 预览。\n\
             Refused: --force --yes permanently deletes everything with no prompt.\n\
             Add --really if you are sure, or use --dry-run to preview first."
        );
        return ExitCode::from(2);
    }

    // 解析 --older-than（尽早失败，给清晰报错）
    let threshold_secs = match &cli.older_than {
        Some(s) => match filter::parse_duration(s) {
            Ok(secs) => secs,
            Err(e) => {
                eprintln!("错误：--older-than 解析失败：{e} / Failed to parse --older-than: {e}");
                return ExitCode::from(2);
            }
        },
        None => 0,
    };

    // 默认扫当前目录
    let raw_roots = if cli.paths.is_empty() {
        vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))]
    } else {
        cli.paths.clone()
    };

    // 关键：扫描前把所有根目录转成绝对路径。否则用户在当前目录跑
    // `reclaim .`，扫出来的全是 `.\foo\node_modules` 这类相对路径，
    // 会被 deleter 的「必须绝对路径」安全关卡全部拒绝——工具一件都删不掉。
    // 用 std::path::absolute（纯词法，不解析符号链接、不加 \\?\ 前缀）。
    let roots: Vec<PathBuf> = raw_roots
        .iter()
        .map(|r| std::path::absolute(r).unwrap_or_else(|_| r.clone()))
        .collect();

    // ---- 阶段 1：扫描 ----
    eprintln!(
        "正在扫描 {} 个根目录… / Scanning {} root(s)…",
        roots.len(),
        roots.len()
    );
    let mut findings = scanner::scan(&roots);

    if findings.is_empty() {
        eprintln!("没有发现可清理的垃圾目录。干净 ヽ(=^･ω･^=)丿 / Nothing to clean. All tidy.");
        return ExitCode::SUCCESS;
    }

    // ---- 阶段 2：算大小 + 记录 mtime ----
    eprintln!(
        "发现 {} 项，正在计算大小… / Found {} item(s), measuring size…",
        findings.len(),
        findings.len()
    );
    sizer::measure_all(&mut findings);

    // ---- 阶段 3：陈旧度过滤 ----
    let before = findings.len();
    findings = filter::filter_stale(findings, sizer::now_secs(), threshold_secs);
    if threshold_secs > 0 {
        eprintln!(
            "陈旧度过滤：{before} 项中保留 {} 项（滤掉最近还在用的） / \
             Staleness filter: kept {} of {before}",
            findings.len(),
            findings.len()
        );
    }

    if findings.is_empty() {
        eprintln!("过滤后没有符合条件的目录。/ Nothing left after filtering.");
        return ExitCode::SUCCESS;
    }

    // ---- 阶段 4a：JSON 输出（不进 TUI）----
    if cli.json {
        println!("{}", report::to_json(&findings));
        return ExitCode::SUCCESS;
    }

    let mode = if cli.force {
        DeleteMode::Permanent
    } else {
        DeleteMode::Trash
    };
    let home = deleter::detect_home();

    // ---- 阶段 4b：选择要删除的项 ----
    // --yes：跳过 TUI，删全部；否则进交互界面勾选。
    let to_delete = if cli.yes {
        findings
    } else {
        // TUI 在部分终端（VS Code 集成终端、PowerShell ISE、某些 SSH 会话）
        // 渲染可能异常。进界面前给一条退路提示，避免新手卡在花屏界面里。
        eprintln!(
            "提示：如遇终端显示异常，请改用 `--yes --dry-run` 先预览。\n\
             Tip: if the display looks broken, try `--yes --dry-run` to preview instead."
        );
        match tui::run(findings) {
            Ok(Outcome::Quit) => {
                eprintln!("已取消，未删除任何内容。/ Cancelled, nothing deleted.");
                return ExitCode::SUCCESS;
            }
            Ok(Outcome::Delete(sel)) => sel,
            Err(e) => {
                eprintln!("TUI 出错：{e} / TUI error: {e}");
                return ExitCode::FAILURE;
            }
        }
    };

    // ---- 阶段 5：删除 ----
    delete_all(&to_delete, mode, cli.dry_run, home.as_deref())
}

/// 执行删除并打印逐项结果。返回进程退出码。
fn delete_all(
    findings: &[reclaim::scanner::Finding],
    mode: DeleteMode,
    dry_run: bool,
    home: Option<&std::path::Path>,
) -> ExitCode {
    let action = match (dry_run, mode) {
        (true, _) => "将删除(预演) / would delete",
        (false, DeleteMode::Trash) => "已移入回收站 / trashed",
        (false, DeleteMode::Permanent) => "已永久删除 / deleted",
    };

    let mut freed: u64 = 0;
    let mut ok = 0usize;
    let mut failed = 0usize;

    for f in findings {
        match deleter::delete_one(&f.path, mode, dry_run, home) {
            Ok(deleted) => {
                if deleted || dry_run {
                    freed = freed.saturating_add(f.size);
                    ok += 1;
                    println!(
                        "{action}  {:>10}  {}",
                        format_size(f.size, DECIMAL),
                        f.path.display()
                    );
                }
            }
            Err(e) => {
                failed += 1;
                eprintln!("跳过 / skipped  {}：{e}", f.path.display());
            }
        }
    }

    let verb = if dry_run { "预计可释放" } else { "释放" };
    let verb_en = if dry_run { "would free" } else { "freed" };
    let failed_zh = if failed > 0 {
        format!("，{failed} 项失败")
    } else {
        String::new()
    };
    let failed_en = if failed > 0 {
        format!(", {failed} failed")
    } else {
        String::new()
    };
    let size = format_size(freed, DECIMAL);
    let dry_note_zh = if dry_run {
        "（预演模式，未实际删除）"
    } else {
        ""
    };
    let dry_note_en = if dry_run {
        " (dry-run, nothing deleted)"
    } else {
        ""
    };
    eprintln!(
        "\n完成：{ok} 项{failed_zh}，{verb} {size}。{dry_note_zh}\n\
         Done: {ok} item(s){failed_en}, {verb_en} {size}.{dry_note_en}"
    );

    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
