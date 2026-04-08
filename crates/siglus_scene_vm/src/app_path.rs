use std::ffi::OsStr;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};

pub fn resolve_app_base_path() -> Result<PathBuf> {
    if std::env::var_os("SIG_TEST").as_deref() == Some(OsStr::new("1")) {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        return Ok(manifest_dir.join("testcase"));
    }

    let exe_path = std::env::current_exe().context("resolve current executable path")?;
    let exe_dir = exe_path
        .parent()
        .ok_or_else(|| anyhow!("executable path has no parent: {}", exe_path.display()))?;
    Ok(exe_dir.to_path_buf())
}
