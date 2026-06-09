//! 删除：整个工具最危险的部分，安全校验优先于一切。
//!
//! 删除哲学：**默认可恢复，越界即拒绝**。
//!
//! 在真正删任何东西之前，每个路径都要过四道关卡（`safety_check`，纯函数）：
//! 1. 必须是绝对路径——相对路径来源不明，一律拒绝。
//! 2. basename 必须是已知垃圾目录名（复用 `rules::is_candidate`）。这是
//!    纵深防御的核心：即便上游逻辑有 bug、把 `src` 这种源码目录传了进来，
//!    这一关也会拦下，从根本上保证「能删到的只可能是垃圾目录名」。
//! 3. 深度下限——拒绝盘符根的直接子目录（`C:\Windows`、`D:\foo`），
//!    正常的项目产物目录都嵌在若干层之下。
//! 4. 系统路径黑名单 + 用户主目录本身——大小写不敏感地拒绝 Windows、
//!    Program Files、System32 等关键系统目录，以及用户主目录根。
//!
//! 删除方式：默认走系统回收站（`trash` crate，可恢复）；只有显式 `--force`
//! 才永久删除。`dry_run` 模式下一律不动手，只汇报将要发生什么。

use crate::rules;
use std::path::{Component, Path, PathBuf};

/// 删除方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteMode {
    /// 移入系统回收站，可恢复。默认。
    Trash,
    /// 永久删除，不可恢复。仅在 `--force` 时启用。
    Permanent,
}

/// 安全校验失败的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafetyError {
    /// 不是绝对路径。
    NotAbsolute,
    /// basename 不是已知的垃圾目录名。
    NotJunkName(String),
    /// 路径太浅（盘符根的直接子目录）。
    TooShallow,
    /// 命中系统路径黑名单。
    SystemPath(String),
    /// 正好是用户主目录本身。
    UserHomeRoot,
}

impl std::fmt::Display for SafetyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SafetyError::NotAbsolute => write!(f, "不是绝对路径"),
            SafetyError::NotJunkName(n) => write!(f, "basename `{n}` 不是已知垃圾目录名"),
            SafetyError::TooShallow => write!(f, "路径过浅，疑似系统/根目录"),
            SafetyError::SystemPath(p) => write!(f, "命中系统路径黑名单：{p}"),
            SafetyError::UserHomeRoot => write!(f, "这是用户主目录本身"),
        }
    }
}

/// 系统关键目录名（小写比较）。路径任意一段命中即拒绝。
const SYSTEM_DIR_NAMES: &[&str] = &[
    "windows",
    "program files",
    "program files (x86)",
    "programdata",
    "system32",
    "syswow64",
    "$recycle.bin",
    "system volume information",
];

/// 提取路径中的「普通段」（排除盘符前缀与根）的小写形式。
fn normal_components_lower(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(os) => os.to_str().map(|s| s.to_ascii_lowercase()),
            _ => None,
        })
        .collect()
}

/// 四道关卡的纯函数实现。`home` 注入用户主目录，便于单测脱离真实环境。
///
/// 通过返回 `Ok(())`，否则返回具体的拒绝原因。
pub fn safety_check(path: &Path, home: Option<&Path>) -> Result<(), SafetyError> {
    // 关卡 1：绝对路径
    if !path.is_absolute() {
        return Err(SafetyError::NotAbsolute);
    }

    // 关卡 2：basename 必须是已知垃圾目录名
    let base = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !rules::is_candidate(base) {
        return Err(SafetyError::NotJunkName(base.to_string()));
    }

    let normals = normal_components_lower(path);

    // 关卡 3：深度下限。盘符根直下只有 1 个普通段（如 C:\Windows），
    // 垃圾目录正常都嵌得更深，要求至少 2 段。
    if normals.len() < 2 {
        return Err(SafetyError::TooShallow);
    }

    // 关卡 4a：系统路径黑名单（任意一段命中即拒）
    for seg in &normals {
        if SYSTEM_DIR_NAMES.contains(&seg.as_str()) {
            return Err(SafetyError::SystemPath(seg.clone()));
        }
    }

    // 关卡 4b：用户主目录本身（规范化后逐段比较）
    if let Some(home) = home
        && paths_equal_ignore_case(path, home)
    {
        return Err(SafetyError::UserHomeRoot);
    }

    Ok(())
}

/// Windows 下大小写不敏感地比较两个路径是否指向同一位置（按普通段比较）。
fn paths_equal_ignore_case(a: &Path, b: &Path) -> bool {
    normal_components_lower(a) == normal_components_lower(b)
}

/// 删除单个路径。`dry_run` 为真时只校验+汇报，不实际删除。
///
/// 返回 `Ok(true)` 表示已删除，`Ok(false)` 表示 dry-run 下跳过实删，
/// `Err` 表示安全校验失败或删除操作出错。
pub fn delete_one(
    path: &Path,
    mode: DeleteMode,
    dry_run: bool,
    home: Option<&Path>,
) -> Result<bool, String> {
    // 安全校验永远先行，dry-run 也要校验（提前暴露问题）
    safety_check(path, home).map_err(|e| format!("{} 被安全校验拒绝：{e}", path.display()))?;

    if dry_run {
        return Ok(false);
    }

    match mode {
        DeleteMode::Trash => {
            trash::delete(path).map_err(|e| format!("移入回收站失败：{e}"))?;
        }
        DeleteMode::Permanent => {
            std::fs::remove_dir_all(path).map_err(|e| format!("永久删除失败：{e}"))?;
        }
    }
    Ok(true)
}

/// 探测当前用户主目录。失败返回 None（safety_check 会跳过主目录校验，
/// 但其余三道关卡仍然生效）。
pub fn detect_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn accepts_legit_junk_path() {
        let home = p(r"C:\Users\me");
        let ok = p(r"C:\Users\me\proj\node_modules");
        assert!(safety_check(&ok, Some(&home)).is_ok());
    }

    #[test]
    fn rejects_relative_path() {
        assert_eq!(
            safety_check(Path::new(r"proj\node_modules"), None),
            Err(SafetyError::NotAbsolute)
        );
    }

    #[test]
    fn rejects_non_junk_basename() {
        // src 不是垃圾目录名，即便路径深、绝对，也必须拒绝
        let path = p(r"C:\Users\me\proj\src");
        match safety_check(&path, None) {
            Err(SafetyError::NotJunkName(n)) => assert_eq!(n, "src"),
            other => panic!("应拒绝非垃圾名，实际：{other:?}"),
        }
    }

    #[test]
    fn rejects_drive_root_child() {
        // C:\node_modules —— 只有 1 个普通段，太浅
        let path = p(r"C:\node_modules");
        assert_eq!(safety_check(&path, None), Err(SafetyError::TooShallow));
    }

    #[test]
    fn rejects_system_path() {
        // 即便 basename 凑巧是垃圾名，路径里有 Windows 段也要拒
        let path = p(r"C:\Windows\System32\node_modules");
        match safety_check(&path, None) {
            Err(SafetyError::SystemPath(_)) => {}
            other => panic!("应命中系统黑名单，实际：{other:?}"),
        }
    }

    #[test]
    fn rejects_program_files() {
        let path = p(r"C:\Program Files\app\node_modules");
        match safety_check(&path, None) {
            Err(SafetyError::SystemPath(seg)) => assert_eq!(seg, "program files"),
            other => panic!("应命中 Program Files，实际：{other:?}"),
        }
    }

    #[test]
    fn rejects_user_home_itself() {
        // 主目录本身 basename 通常不是垃圾名，会先被关卡 2 拦下；
        // 这里构造一个 basename 恰为垃圾名、且整体等于 home 的极端情况，
        // 验证关卡 4b 生效。
        let home = p(r"C:\Users\target");
        // 注意：basename "target" 是候选名，深度也够，靠 home 比较拦截
        assert_eq!(
            safety_check(&home, Some(&home)),
            Err(SafetyError::UserHomeRoot)
        );
    }

    #[test]
    fn system_check_is_case_insensitive() {
        let path = p(r"c:\WINDOWS\foo\node_modules");
        assert!(matches!(
            safety_check(&path, None),
            Err(SafetyError::SystemPath(_))
        ));
    }

    #[test]
    fn dry_run_validates_but_does_not_delete() {
        // dry-run 对一个不存在但合法的路径：应返回 Ok(false)（跳过实删），
        // 而不是因为路径不存在而报错——因为根本没碰文件系统。
        let home = p(r"C:\Users\me");
        let path = p(r"C:\Users\me\proj\node_modules");
        let r = delete_one(&path, DeleteMode::Trash, true, Some(&home));
        assert_eq!(r, Ok(false));
    }

    #[test]
    fn dry_run_still_rejects_unsafe() {
        // dry-run 也要跑安全校验：危险路径即便不实删也要报错
        let path = p(r"C:\Windows\node_modules");
        let r = delete_one(&path, DeleteMode::Trash, true, None);
        assert!(r.is_err());
    }
}
