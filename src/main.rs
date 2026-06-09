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
#[derive(Parser, Debug)]
#[command(name = "reclaim", version, about, long_about = None)]
struct Cli {
    /// 要扫描的根目录（可多个）。省略时扫描当前目录。
    #[arg(value_name = "PATH")]
    paths: Vec<PathBuf>,

    /// 只清理「最近一次改动距今 ≥ 该时长」的目录。
    /// 语法：数字 + 单位 s/m/h/d/w，例如 30d、2w。
    /// 不指定则不按陈旧度过滤。
    #[arg(long, value_name = "DURATION")]
    older_than: Option<String>,

    /// 输出 JSON 给脚本消费，不进入交互界面。
    #[arg(long)]
    json: bool,

    /// 预演模式：只显示将要删除什么，不真正删除。
    #[arg(long)]
    dry_run: bool,

    /// 永久删除而非移入回收站（不可恢复，慎用）。
    #[arg(long)]
    force: bool,

    /// 跳过 TUI，直接删除所有扫描到的项（配合 --older-than 使用，谨慎）。
    /// 仍受安全校验保护。
    #[arg(long)]
    yes: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // 解析 --older-than（尽早失败，给清晰报错）
    let threshold_secs = match &cli.older_than {
        Some(s) => match filter::parse_duration(s) {
            Ok(secs) => secs,
            Err(e) => {
                eprintln!("错误：--older-than 解析失败：{e}");
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
    eprintln!("正在扫描 {} 个根目录…", roots.len());
    let mut findings = scanner::scan(&roots);

    if findings.is_empty() {
        eprintln!("没有发现可清理的垃圾目录。干净 ヽ(=^･ω･^=)丿");
        return ExitCode::SUCCESS;
    }

    // ---- 阶段 2：算大小 + 记录 mtime ----
    eprintln!("发现 {} 项，正在计算大小…", findings.len());
    sizer::measure_all(&mut findings);

    // ---- 阶段 3：陈旧度过滤 ----
    let before = findings.len();
    findings = filter::filter_stale(findings, sizer::now_secs(), threshold_secs);
    if threshold_secs > 0 {
        eprintln!(
            "陈旧度过滤：{} 项中保留 {} 项（滤掉最近还在用的）",
            before,
            findings.len()
        );
    }

    if findings.is_empty() {
        eprintln!("过滤后没有符合条件的目录。");
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
        match tui::run(findings) {
            Ok(Outcome::Quit) => {
                eprintln!("已取消，未删除任何内容。");
                return ExitCode::SUCCESS;
            }
            Ok(Outcome::Delete(sel)) => sel,
            Err(e) => {
                eprintln!("TUI 出错：{e}");
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
        (true, _) => "将删除（预演）",
        (false, DeleteMode::Trash) => "已移入回收站",
        (false, DeleteMode::Permanent) => "已永久删除",
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
                eprintln!("跳过  {}：{e}", f.path.display());
            }
        }
    }

    let verb = if dry_run { "预计可释放" } else { "释放" };
    eprintln!(
        "\n完成：{ok} 项{}，{verb} {}。{}",
        if failed > 0 {
            format!("，{failed} 项失败")
        } else {
            String::new()
        },
        format_size(freed, DECIMAL),
        if dry_run {
            "（预演模式，未实际删除）"
        } else {
            ""
        }
    );

    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
