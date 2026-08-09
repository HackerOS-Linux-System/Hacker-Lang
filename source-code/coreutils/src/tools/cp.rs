use anyhow::{bail, Result};
use std::fs;

use std::path::Path;
use crate::BuiltinResult;

pub fn builtin_cp(args: &[String]) -> Result<BuiltinResult> {
    let mut recursive = false;
    let mut files: Vec<String> = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-r" | "-R" | "--recursive" => recursive = true,
            s => files.push(s.to_string()),
        }
    }
    if files.len() < 2 { bail!("[/> cp] Użycie: cp [-r] src dst"); }
    let dst = files.last().unwrap().clone();
    let srcs = &files[..files.len()-1];
    let dst_path = Path::new(&dst);

    for src in srcs {
        let src_path = Path::new(src);
        if src_path.is_dir() && recursive {
            copy_dir_recursive(src_path, &dst_path.join(src_path.file_name().unwrap()))?;
        } else if src_path.is_file() {
            let dest = if dst_path.is_dir() {
                dst_path.join(src_path.file_name().unwrap())
            } else {
                dst_path.to_path_buf()
            };
            fs::copy(src_path, &dest)?;
        } else {
            eprintln!("[/> cp] {} nie istnieje", src);
            return Ok(BuiltinResult::err(1));
        }
    }
    Ok(BuiltinResult::ok())
}

pub fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest = dst.join(entry.file_name());
        if path.is_dir() { copy_dir_recursive(&path, &dest)?; }
        else { fs::copy(&path, &dest)?; }
    }
    Ok(())
}
