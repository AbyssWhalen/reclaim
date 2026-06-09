//! 端到端集成测试：在系统临时目录里造一棵假的多语言项目树，
//! 跑完整的 scan → size → filter 流程，断言「只命中垃圾、放过源码」。
//!
//! 安全：所有文件都建在 `std::env::temp_dir()` 下的一个唯一子目录里，
//! 测试结束（无论成败）通过 Drop 自动清理。绝不触碰临时目录之外的任何东西。

use reclaim::filter;
use reclaim::scanner::{self, Finding};
use reclaim::sizer;
use std::fs;
use std::path::{Path, PathBuf};

/// 一个测试用的临时沙箱目录，Drop 时递归删除自身。
struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        // 用进程 id + 纳秒时间戳保证唯一，避免并行测试互相踩
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("reclaim-test-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Sandbox { root }
    }

    /// 在沙箱内创建一个文件（含父目录），写入少量内容。
    fn file(&self, rel: &str, content: &[u8]) -> PathBuf {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    /// 在沙箱内创建一个目录（含父目录）。
    fn dir(&self, rel: &str) -> PathBuf {
        let path = self.root.join(rel);
        fs::create_dir_all(&path).unwrap();
        path
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        // 尽力清理；忽略错误（测试已结束，残留临时目录无伤大雅）
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// 扫描结果里命中的路径集合（转成相对沙箱根的字符串，便于断言）。
fn hit_relpaths(findings: &[Finding], root: &Path) -> Vec<String> {
    let mut v: Vec<String> = findings
        .iter()
        .map(|f| {
            f.path
                .strip_prefix(root)
                .unwrap_or(&f.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    v.sort();
    v
}

#[test]
fn finds_junk_and_spares_source() {
    let sb = Sandbox::new("mixed");

    // ---- Rust 项目：有 Cargo.toml + target/（应命中 target）----
    sb.file("rust-proj/Cargo.toml", b"[package]\nname=\"x\"\n");
    sb.file("rust-proj/src/main.rs", b"fn main(){}");
    sb.file("rust-proj/target/debug/app.exe", b"binarydata");

    // ---- Node 项目：node_modules/（自证型，应命中）----
    sb.file("node-proj/package.json", b"{}");
    sb.file(
        "node-proj/node_modules/lodash/index.js",
        b"module.exports={}",
    );

    // ---- 裸 target/：旁边没有 Cargo.toml（必须放过！）----
    sb.file(
        "not-a-rust-proj/target/data.txt",
        b"this is user data, keep it",
    );
    sb.file("not-a-rust-proj/readme.txt", b"hello");

    // ---- 纯源码目录 src/（必须放过）----
    sb.file("plain/src/lib.rs", b"// source code, never delete");

    // ---- Python 缓存 __pycache__/（自证型，应命中）----
    sb.file("py-proj/main.py", b"print(1)");
    sb.file("py-proj/__pycache__/main.cpython-312.pyc", b"\x00bytecode");

    let findings = scanner::scan(std::slice::from_ref(&sb.root));
    let hits = hit_relpaths(&findings, &sb.root);

    // 必须命中：rust target、node_modules、python __pycache__
    assert!(
        hits.contains(&"rust-proj/target".to_string()),
        "应命中 Rust target，实际命中：{hits:?}"
    );
    assert!(
        hits.contains(&"node-proj/node_modules".to_string()),
        "应命中 node_modules，实际命中：{hits:?}"
    );
    assert!(
        hits.contains(&"py-proj/__pycache__".to_string()),
        "应命中 __pycache__，实际命中：{hits:?}"
    );

    // 必须放过：裸 target（无 Cargo.toml）、源码 src
    assert!(
        !hits.contains(&"not-a-rust-proj/target".to_string()),
        "裸 target 不该被命中（无 Cargo.toml）！实际命中：{hits:?}"
    );
    assert!(
        !hits.iter().any(|h| h.ends_with("/src") || h == "src"),
        "源码目录 src 绝不该被命中！实际命中：{hits:?}"
    );

    // 精确性：本例应恰好命中 3 个
    assert_eq!(
        findings.len(),
        3,
        "应恰好命中 3 处垃圾（rust target / node_modules / __pycache__），实际：{hits:?}"
    );
}

#[test]
fn prune_does_not_descend_into_junk() {
    // node_modules 内部常有嵌套 node_modules；命中后应剪枝，
    // 不该把嵌套的也作为独立项报出来。
    let sb = Sandbox::new("prune");
    sb.file("app/package.json", b"{}");
    sb.file("app/node_modules/a/index.js", b"x");
    sb.file("app/node_modules/a/node_modules/b/index.js", b"y"); // 嵌套

    let findings = scanner::scan(std::slice::from_ref(&sb.root));
    let hits = hit_relpaths(&findings, &sb.root);

    assert_eq!(
        findings.len(),
        1,
        "命中外层 node_modules 后应剪枝，不报嵌套项。实际：{hits:?}"
    );
    assert_eq!(hits[0], "app/node_modules");
}

#[test]
fn sizer_computes_real_sizes() {
    let sb = Sandbox::new("size");
    sb.file("proj/Cargo.toml", b"[package]");
    // target 里放两个已知大小的文件：100 + 200 = 300 字节
    sb.file("proj/target/a.bin", &[0u8; 100]);
    sb.file("proj/target/sub/b.bin", &[0u8; 200]);

    let mut findings = scanner::scan(std::slice::from_ref(&sb.root));
    assert_eq!(findings.len(), 1, "应命中 proj/target");

    sizer::measure_all(&mut findings);
    assert_eq!(findings[0].size, 300, "target 总大小应为 300 字节");
    assert!(
        findings[0].newest_mtime_secs.is_some(),
        "算大小时应同时记录最新 mtime"
    );
}

#[test]
fn stale_filter_removes_recent() {
    // 验证陈旧度过滤：刚创建的目录 mtime 接近 now，
    // 用一个很大的阈值（比如 1 年）应把它全滤掉。
    let sb = Sandbox::new("stale");
    sb.file("proj/Cargo.toml", b"[package]");
    sb.file("proj/target/x.bin", b"data");

    let mut findings = scanner::scan(std::slice::from_ref(&sb.root));
    sizer::measure_all(&mut findings);
    assert_eq!(findings.len(), 1);

    let now = sizer::now_secs();
    let one_year = 365 * 24 * 60 * 60;
    // 刚建的文件，距今远不到一年 → 应被滤掉
    let kept = filter::filter_stale(findings, now, one_year);
    assert!(
        kept.is_empty(),
        "刚创建的目录在『一年以上未动』过滤下应被全部滤掉"
    );
}

#[test]
fn empty_dir_yields_nothing() {
    let sb = Sandbox::new("empty");
    sb.dir("just/some/empty/dirs");
    let findings = scanner::scan(std::slice::from_ref(&sb.root));
    assert!(findings.is_empty(), "没有任何垃圾时应返回空");
}
