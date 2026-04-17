//! 14 filesystem tools: read, read_many, write, edit, write_pdf is omitted (PDF
//! gen on Linux without a heavy dep is awkward; reserved for v0.2.0), list,
//! stat, copy, move, make_dir, delete, watch_once, xattr_get, xattr_set.

use crate::error::{ok_json, ToolError};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use linux_mcp_core::{PathMode, PathPolicy};
use rmcp::model::CallToolResult;
use std::path::PathBuf;
use std::time::SystemTime;

pub const MAX_READ_BYTES: usize = 10 * 1024 * 1024;
pub const DEFAULT_READ_BYTES: usize = 1 * 1024 * 1024;
pub const MAX_LIST_ENTRIES: usize = 5_000;
pub const DEFAULT_LIST_ENTRIES: usize = 500;
pub const MAX_WRITE_BYTES: usize = 50 * 1024 * 1024;
pub const MAX_BATCH_FILES: usize = 50;
pub const MAX_BATCH_TOTAL: usize = 10 * 1024 * 1024;

pub fn check(policy: &PathPolicy, path: &str, mode: PathMode) -> Result<PathBuf, ToolError> {
    Ok(policy.check(path, mode)?)
}

// ----- fs_read -----
pub async fn fs_read(
    policy: &PathPolicy,
    path: &str,
    encoding: Option<&str>,
    offset: Option<i64>,
    max_bytes: Option<i64>,
) -> Result<CallToolResult, ToolError> {
    use std::io::{Read, Seek, SeekFrom};

    let resolved = check(policy, path, PathMode::Read)?;
    let offset = offset.unwrap_or(0).max(0) as u64;
    let limit = (max_bytes.unwrap_or(DEFAULT_READ_BYTES as i64))
        .clamp(1, MAX_READ_BYTES as i64) as usize;

    let mut file = std::fs::File::open(&resolved)
        .map_err(|e| ToolError::coded("fs_read_failed", e.to_string()))?;
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; limit];
    let n = file.read(&mut buf)?;
    buf.truncate(n);

    let total_size = std::fs::metadata(&resolved).map(|m| m.len()).unwrap_or(n as u64);
    let truncated = (offset + n as u64) < total_size;

    let mut out = serde_json::json!({
        "path": resolved.display().to_string(),
        "size_total": total_size,
        "bytes_returned": n,
        "offset": offset,
        "truncated": truncated,
    });
    match encoding.unwrap_or("text") {
        "base64" => {
            out["base64"] = B64.encode(&buf).into();
        }
        _ => match String::from_utf8(buf.clone()) {
            Ok(s) => {
                out["text"] = s.into();
            }
            Err(_) => {
                return Err(ToolError::coded(
                    "fs_not_utf8",
                    "file is not valid UTF-8 — re-call with as='base64'",
                ))
            }
        },
    }
    Ok(ok_json(out))
}

// ----- fs_read_many -----
pub async fn fs_read_many(
    policy: &PathPolicy,
    paths: &[String],
    encoding: Option<&str>,
) -> Result<CallToolResult, ToolError> {
    if paths.len() > MAX_BATCH_FILES {
        return Err(ToolError::coded(
            "batch_too_big",
            format!("got {} paths, cap is {}", paths.len(), MAX_BATCH_FILES),
        ));
    }
    let encoding = encoding.unwrap_or("text");
    let mut total_bytes = 0usize;
    let mut results = Vec::with_capacity(paths.len());

    for raw in paths {
        if total_bytes >= MAX_BATCH_TOTAL {
            results.push(serde_json::json!({
                "path": raw,
                "error": "batch_total_cap_reached",
                "skipped": true,
            }));
            continue;
        }
        let entry = match policy.check(raw, PathMode::Read) {
            Err(e) => serde_json::json!({ "path": raw, "error": e.code(), "message": e.to_string() }),
            Ok(resolved) => match std::fs::read(&resolved) {
                Err(e) => serde_json::json!({ "path": raw, "error": "read_failed", "message": e.to_string() }),
                Ok(data) => {
                    let take = data.len().min(MAX_BATCH_TOTAL - total_bytes);
                    let payload = &data[..take];
                    total_bytes += take;
                    let mut entry = serde_json::json!({
                        "path": resolved.display().to_string(),
                        "size": data.len(),
                        "bytes_returned": take,
                    });
                    if encoding == "base64" {
                        entry["base64"] = B64.encode(payload).into();
                    } else {
                        match std::str::from_utf8(payload) {
                            Ok(s) => entry["text"] = s.into(),
                            Err(_) => {
                                entry["error"] = "not_utf8".into();
                                entry["base64"] = B64.encode(payload).into();
                            }
                        }
                    }
                    entry
                }
            },
        };
        results.push(entry);
    }

    Ok(ok_json(serde_json::json!({
        "count": results.len(),
        "total_bytes": total_bytes,
        "files": results,
    })))
}

// ----- fs_write -----
pub async fn fs_write(
    policy: &PathPolicy,
    path: &str,
    text: Option<&str>,
    base64: Option<&str>,
    mode: Option<&str>,
) -> Result<CallToolResult, ToolError> {
    use std::io::Write;
    let resolved = check(policy, path, PathMode::Write)?;

    let data: Vec<u8> = if let Some(t) = text {
        t.as_bytes().to_vec()
    } else if let Some(b) = base64 {
        B64.decode(b)
            .map_err(|e| ToolError::coded("bad_base64", e.to_string()))?
    } else {
        return Err(ToolError::coded("missing_arg", "provide one of: text, base64"));
    };

    if data.len() > MAX_WRITE_BYTES {
        return Err(ToolError::coded(
            "fs_too_big",
            format!("payload {} > cap {} bytes", data.len(), MAX_WRITE_BYTES),
        ));
    }

    let mode = mode.unwrap_or("create");
    let exists = resolved.exists();
    match mode {
        "create" => {
            if exists {
                return Err(ToolError::coded("fs_exists", "path exists; use mode='overwrite' or 'append'"));
            }
            atomic_write(&resolved, &data)?;
        }
        "overwrite" => atomic_write(&resolved, &data)?,
        "append" => {
            if exists {
                let mut f = std::fs::OpenOptions::new().append(true).open(&resolved)?;
                f.write_all(&data)?;
            } else {
                atomic_write(&resolved, &data)?;
            }
        }
        _ => return Err(ToolError::coded("bad_mode", format!("unknown mode: {mode}"))),
    }

    Ok(ok_json(serde_json::json!({
        "path": resolved.display().to_string(),
        "bytes_written": data.len(),
        "mode": mode,
    })))
}

fn atomic_write(target: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    let parent = target.parent().unwrap_or_else(|| std::path::Path::new("."));
    let file_name = target.file_name().unwrap_or_default().to_string_lossy().to_string();
    let tmp = parent.join(format!(".{file_name}.tmp.{}", std::process::id()));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, target)
}

// ----- fs_edit -----
pub async fn fs_edit(
    policy: &PathPolicy,
    path: &str,
    find: &str,
    replace: &str,
    occurrences: Option<&str>,
    expect_count: Option<i64>,
) -> Result<CallToolResult, ToolError> {
    let resolved = check(policy, path, PathMode::Write)?;
    let original = std::fs::read_to_string(&resolved)
        .map_err(|e| ToolError::coded("fs_read_failed", e.to_string()))?;
    let count = original.matches(find).count();

    let (updated, made) = match occurrences.unwrap_or("all") {
        "all" => (original.replace(find, replace), count),
        "first" => match original.find(find) {
            Some(i) => (
                format!("{}{}{}", &original[..i], replace, &original[i + find.len()..]),
                if count > 0 { 1 } else { 0 },
            ),
            None => (original.clone(), 0),
        },
        "last" => match original.rfind(find) {
            Some(i) => (
                format!("{}{}{}", &original[..i], replace, &original[i + find.len()..]),
                if count > 0 { 1 } else { 0 },
            ),
            None => (original.clone(), 0),
        },
        other => {
            return Err(ToolError::coded(
                "bad_mode",
                format!("occurrences must be all|first|last (got {other})"),
            ))
        }
    };

    if let Some(exp) = expect_count {
        if exp != made as i64 {
            return Err(ToolError::coded(
                "expect_count_mismatch",
                format!("expected {exp}, would have made {made} — refusing to write"),
            ));
        }
    }

    atomic_write(&resolved, updated.as_bytes())?;
    Ok(ok_json(serde_json::json!({
        "path": resolved.display().to_string(),
        "matches_found": count,
        "replacements_made": made,
        "new_size": updated.len(),
    })))
}

// ----- fs_list -----
pub async fn fs_list(
    policy: &PathPolicy,
    path: &str,
    recursive: Option<bool>,
    max_entries: Option<i64>,
    glob_pattern: Option<&str>,
) -> Result<CallToolResult, ToolError> {
    let resolved = check(policy, path, PathMode::Read)?;
    let limit = max_entries.unwrap_or(DEFAULT_LIST_ENTRIES as i64).clamp(1, MAX_LIST_ENTRIES as i64) as usize;
    let mut out: Vec<serde_json::Value> = Vec::new();
    let mut truncated = false;

    let globber = glob_pattern.map(|p| glob::Pattern::new(p)).transpose()
        .map_err(|e| ToolError::coded("bad_glob", e.to_string()))?;

    let walker: Box<dyn Iterator<Item = std::path::PathBuf>> = if recursive.unwrap_or(false) {
        Box::new(walkdir::WalkDir::new(&resolved).into_iter().filter_map(|e| e.ok()).map(|e| e.into_path()))
    } else {
        let read = std::fs::read_dir(&resolved)
            .map_err(|e| ToolError::coded("fs_list_failed", e.to_string()))?;
        Box::new(read.filter_map(|r| r.ok()).map(|e| e.path()))
    };

    for p in walker {
        if out.len() >= limit {
            truncated = true;
            break;
        }
        let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if let Some(g) = &globber {
            if !g.matches(&name) {
                continue;
            }
        }
        let meta = std::fs::symlink_metadata(&p).ok();
        let kind = match meta.as_ref() {
            Some(m) if m.is_symlink() => "symlink",
            Some(m) if m.is_dir() => "dir",
            Some(_) => "file",
            None => "unknown",
        };
        let mut obj = serde_json::json!({
            "name": name,
            "path": p.display().to_string(),
            "kind": kind,
        });
        if let Some(m) = &meta {
            obj["size"] = m.len().into();
            if let Ok(mtime) = m.modified() {
                obj["mtime"] = system_time_to_iso(mtime).into();
            }
        }
        out.push(obj);
    }

    Ok(ok_json(serde_json::json!({
        "path": resolved.display().to_string(),
        "count": out.len(),
        "truncated": truncated,
        "entries": out,
    })))
}

// ----- fs_stat -----
pub async fn fs_stat(policy: &PathPolicy, path: &str) -> Result<CallToolResult, ToolError> {
    use std::os::unix::fs::MetadataExt;
    let resolved = check(policy, path, PathMode::Read)?;
    let meta = std::fs::symlink_metadata(&resolved)
        .map_err(|e| ToolError::coded("fs_stat_failed", e.to_string()))?;
    let kind = if meta.is_symlink() {
        "symlink"
    } else if meta.is_dir() {
        "dir"
    } else {
        "file"
    };
    let symlink_target = if meta.is_symlink() {
        std::fs::read_link(&resolved).ok().map(|p| p.display().to_string())
    } else {
        None
    };
    let xattrs: Vec<String> = xattr::list(&resolved)
        .map(|iter| iter.filter_map(|os| os.into_string().ok()).collect())
        .unwrap_or_default();

    let mut obj = serde_json::json!({
        "path": resolved.display().to_string(),
        "kind": kind,
        "size": meta.len(),
        "perms_octal": format!("{:o}", meta.mode() & 0o7777),
        "uid": meta.uid(),
        "gid": meta.gid(),
        "xattrs": xattrs,
    });
    if let Ok(t) = meta.modified() {
        obj["mtime"] = system_time_to_iso(t).into();
    }
    if let Ok(t) = meta.created() {
        obj["ctime"] = system_time_to_iso(t).into();
    }
    if let Ok(t) = meta.accessed() {
        obj["atime"] = system_time_to_iso(t).into();
    }
    if let Some(t) = symlink_target {
        obj["symlink_target"] = t.into();
    }
    Ok(ok_json(obj))
}

// ----- fs_copy / fs_move -----
pub async fn fs_copy(
    policy: &PathPolicy,
    src: &str,
    dst: &str,
    overwrite: Option<bool>,
) -> Result<CallToolResult, ToolError> {
    let s = check(policy, src, PathMode::Read)?;
    let d = check(policy, dst, PathMode::Write)?;
    let overwrite = overwrite.unwrap_or(false);
    if d.exists() {
        if overwrite {
            if d.is_dir() {
                std::fs::remove_dir_all(&d)?;
            } else {
                std::fs::remove_file(&d)?;
            }
        } else {
            return Err(ToolError::coded("fs_exists", "dst exists; pass overwrite=true to replace"));
        }
    }
    if s.is_dir() {
        let opts = fs_extra::dir::CopyOptions::new().overwrite(true).copy_inside(true);
        fs_extra::dir::copy(&s, &d, &opts)
            .map_err(|e| ToolError::coded("fs_copy_failed", e.to_string()))?;
    } else {
        std::fs::copy(&s, &d).map_err(|e| ToolError::coded("fs_copy_failed", e.to_string()))?;
    }
    Ok(ok_json(serde_json::json!({
        "copied": true,
        "src": s.display().to_string(),
        "dst": d.display().to_string(),
    })))
}

pub async fn fs_move(
    policy: &PathPolicy,
    src: &str,
    dst: &str,
    overwrite: Option<bool>,
) -> Result<CallToolResult, ToolError> {
    let s = check(policy, src, PathMode::Write)?;
    let d = check(policy, dst, PathMode::Write)?;
    let overwrite = overwrite.unwrap_or(false);
    if d.exists() {
        if overwrite {
            if d.is_dir() {
                std::fs::remove_dir_all(&d)?;
            } else {
                std::fs::remove_file(&d)?;
            }
        } else {
            return Err(ToolError::coded("fs_exists", "dst exists; pass overwrite=true to replace"));
        }
    }
    std::fs::rename(&s, &d).map_err(|e| ToolError::coded("fs_move_failed", e.to_string()))?;
    Ok(ok_json(serde_json::json!({
        "moved": true,
        "src": s.display().to_string(),
        "dst": d.display().to_string(),
    })))
}

// ----- fs_make_dir -----
pub async fn fs_make_dir(
    policy: &PathPolicy,
    path: &str,
    parents: Option<bool>,
    perms_octal: Option<&str>,
) -> Result<CallToolResult, ToolError> {
    use std::os::unix::fs::PermissionsExt;
    let resolved = check(policy, path, PathMode::Write)?;
    let parents = parents.unwrap_or(true);
    let perms_str = perms_octal.unwrap_or("755");
    let perms = u32::from_str_radix(perms_str, 8)
        .map_err(|_| ToolError::coded("bad_arg", format!("perms_octal must be octal like '755', got '{perms_str}'")))?;

    if parents {
        std::fs::create_dir_all(&resolved)?;
    } else {
        std::fs::create_dir(&resolved)?;
    }
    std::fs::set_permissions(&resolved, std::fs::Permissions::from_mode(perms))?;
    Ok(ok_json(serde_json::json!({
        "created": resolved.display().to_string(),
        "perms_octal": perms_str,
    })))
}

// ----- fs_delete -----
pub async fn fs_delete(
    policy: &PathPolicy,
    path: &str,
    permanent: Option<bool>,
) -> Result<CallToolResult, ToolError> {
    let resolved = check(policy, path, PathMode::Write)?;
    let permanent = permanent.unwrap_or(false);
    if permanent {
        if resolved.is_dir() {
            std::fs::remove_dir_all(&resolved)?;
        } else {
            std::fs::remove_file(&resolved)?;
        }
        return Ok(ok_json(serde_json::json!({
            "deleted": true,
            "permanent": true,
            "path": resolved.display().to_string(),
        })));
    }
    // FreeDesktop trash: $XDG_DATA_HOME/Trash, fall back to ~/.local/share/Trash
    let trash_root = trash_dir();
    let files_dir = trash_root.join("files");
    let info_dir = trash_root.join("info");
    std::fs::create_dir_all(&files_dir)?;
    std::fs::create_dir_all(&info_dir)?;

    let stem = resolved.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "deleted".into());
    let mut target = files_dir.join(&stem);
    let mut counter = 1;
    while target.exists() {
        target = files_dir.join(format!("{stem}.{counter}"));
        counter += 1;
    }
    std::fs::rename(&resolved, &target)
        .map_err(|e| ToolError::coded("trash_move_failed", e.to_string()))?;
    let info_file = info_dir.join(format!("{}.trashinfo", target.file_name().unwrap().to_string_lossy()));
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    let info = format!("[Trash Info]\nPath={}\nDeletionDate={}\n", resolved.display(), now);
    let _ = std::fs::write(&info_file, info);
    Ok(ok_json(serde_json::json!({
        "deleted": true,
        "permanent": false,
        "trash_path": target.display().to_string(),
    })))
}

fn trash_dir() -> PathBuf {
    if let Ok(x) = std::env::var("XDG_DATA_HOME") {
        if !x.is_empty() {
            return PathBuf::from(x).join("Trash");
        }
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into())).join(".local/share/Trash")
}

// ----- fs_watch_once -----
pub async fn fs_watch_once(
    policy: &PathPolicy,
    path: &str,
    timeout_ms: Option<i64>,
) -> Result<CallToolResult, ToolError> {
    use notify::{RecommendedWatcher, RecursiveMode, Watcher};
    use std::sync::mpsc::channel;

    let resolved = check(policy, path, PathMode::Read)?;
    let timeout_ms = timeout_ms.unwrap_or(30_000).clamp(100, 600_000) as u64;

    let (tx, rx) = channel();
    let mut watcher: RecommendedWatcher = notify::Watcher::new(
        move |res| {
            let _ = tx.send(res);
        },
        notify::Config::default(),
    )
    .map_err(|e| ToolError::coded("watch_init_failed", e.to_string()))?;
    watcher
        .watch(&resolved, RecursiveMode::Recursive)
        .map_err(|e| ToolError::coded("watch_failed", e.to_string()))?;

    let result = tokio::task::spawn_blocking(move || {
        rx.recv_timeout(std::time::Duration::from_millis(timeout_ms)).ok()
    })
    .await
    .ok()
    .flatten();

    let mut changed_paths: Vec<String> = Vec::new();
    let mut timed_out = true;
    if let Some(Ok(event)) = result {
        timed_out = false;
        for p in event.paths {
            changed_paths.push(p.display().to_string());
        }
    }
    Ok(ok_json(serde_json::json!({
        "path": resolved.display().to_string(),
        "changed_paths": changed_paths,
        "timed_out": timed_out,
    })))
}

// ----- fs_xattr_get / fs_xattr_set -----
pub async fn fs_xattr_get(
    policy: &PathPolicy,
    path: &str,
    name: &str,
) -> Result<CallToolResult, ToolError> {
    let resolved = check(policy, path, PathMode::Read)?;
    if name == "*" {
        let names: Vec<String> = xattr::list(&resolved)
            .map(|iter| iter.filter_map(|os| os.into_string().ok()).collect())
            .map_err(|e| ToolError::coded("xattr_list_failed", e.to_string()))?;
        return Ok(ok_json(serde_json::json!({
            "path": resolved.display().to_string(),
            "names": names,
        })));
    }
    match xattr::get(&resolved, name)
        .map_err(|e| ToolError::coded("xattr_failed", e.to_string()))?
    {
        Some(data) => Ok(ok_json(serde_json::json!({
            "path": resolved.display().to_string(),
            "name": name,
            "value_b64": B64.encode(&data),
            "size": data.len(),
        }))),
        None => Err(ToolError::coded(
            "xattr_missing",
            format!("no xattr '{name}' on {}", resolved.display()),
        )),
    }
}

pub async fn fs_xattr_set(
    policy: &PathPolicy,
    path: &str,
    name: &str,
    value_text: Option<&str>,
    value_b64: Option<&str>,
) -> Result<CallToolResult, ToolError> {
    let resolved = check(policy, path, PathMode::Write)?;
    let data: Vec<u8> = if let Some(t) = value_text {
        t.as_bytes().to_vec()
    } else if let Some(b) = value_b64 {
        B64.decode(b).map_err(|e| ToolError::coded("bad_base64", e.to_string()))?
    } else {
        return Err(ToolError::coded("missing_arg", "provide value_text or value_b64"));
    };
    xattr::set(&resolved, name, &data)
        .map_err(|e| ToolError::coded("xattr_set_failed", e.to_string()))?;
    Ok(ok_json(serde_json::json!({
        "set": true,
        "path": resolved.display().to_string(),
        "name": name,
        "size": data.len(),
    })))
}

fn system_time_to_iso(t: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Utc> = t.into();
    dt.to_rfc3339()
}
