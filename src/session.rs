use std::collections::HashMap;
use std::path::PathBuf;

/// Return the path to Claude Code's sessions directory.
fn sessions_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let dir = PathBuf::from(home).join(".claude").join("sessions");
    if dir.is_dir() { Some(dir) } else { None }
}

/// Scan `~/.claude/sessions/*.json` for session names.
pub fn scan_session_names() -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(dir) = sessions_dir() else {
        return map;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return map;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Some((session_id, name)) = parse_session_file(&path) {
            map.insert(session_id, name);
        }
    }
    map
}

/// Parse a single session JSON file, returning `(sessionId, name)` if both exist.
fn parse_session_file(path: &std::path::Path) -> Option<(String, String)> {
    let content = std::fs::read_to_string(path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&content).ok()?;
    let session_id = val.get("sessionId")?.as_str()?.trim();
    let name = val.get("name")?.as_str()?.trim();
    if session_id.is_empty() || name.is_empty() {
        return None;
    }
    Some((session_id.to_string(), name.to_string()))
}

/// Whether the session still has a live owning process, per
/// `~/.claude/sessions/<pid>.json` (pid cross-checked with the recorded
/// procStart so a reused pid never counts). Only Claude Code writes these
/// files; unknown sessions (codex/opencode) report `false`, so the hook-side
/// occupancy guard never blocks them.
pub(crate) fn session_alive(session_id: &str) -> bool {
    sessions_dir().is_some_and(|dir| session_alive_in(&dir, session_id, pid_is_live))
}

fn session_alive_in(
    dir: &std::path::Path,
    session_id: &str,
    pid_is_live: impl Fn(u32, &str) -> bool,
) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Some((sid, pid, proc_start)) = parse_session_liveness(&path)
            && sid == session_id
            && pid_is_live(pid, &proc_start)
        {
            return true;
        }
    }
    false
}

/// `(sessionId, pid, procStart)` from a sessions/<pid>.json file.
fn parse_session_liveness(path: &std::path::Path) -> Option<(String, u32, String)> {
    let content = std::fs::read_to_string(path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&content).ok()?;
    let session_id = val.get("sessionId")?.as_str()?.trim();
    let pid = val.get("pid")?.as_u64()?;
    let proc_start = val.get("procStart")?.as_str()?.trim();
    if session_id.is_empty() || pid == 0 || proc_start.is_empty() {
        return None;
    }
    Some((session_id.to_string(), pid as u32, proc_start.to_string()))
}

/// /proc/<pid>/stat field 22 (starttime, clock ticks since boot) must equal
/// the recorded procStart. Without /proc (non-Linux) nothing is live.
fn pid_is_live(pid: u32, proc_start: &str) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    // comm ("(claude)") may contain spaces in general; tokens after the
    // closing paren start at stat field 3, so starttime is token 20 there.
    stat.split(')')
        .nth(1)
        .and_then(|rest| rest.split_whitespace().nth(19))
        .is_some_and(|t| t == proc_start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parse_session_file_with_name() {
        let dir = std::env::temp_dir().join("session_test_with_name");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("12345.json");
        fs::write(
            &path,
            r#"{"pid":12345,"sessionId":"abc-def","name":"my-session","cwd":"/tmp"}"#,
        )
        .unwrap();

        let result = parse_session_file(&path);
        assert_eq!(result, Some(("abc-def".into(), "my-session".into())));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_session_file_without_name() {
        let dir = std::env::temp_dir().join("session_test_no_name");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("12345.json");
        fs::write(&path, r#"{"pid":12345,"sessionId":"abc-def","cwd":"/tmp"}"#).unwrap();

        assert!(parse_session_file(&path).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_session_file_empty_name() {
        let dir = std::env::temp_dir().join("session_test_empty_name");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("12345.json");
        fs::write(
            &path,
            r#"{"pid":12345,"sessionId":"abc-def","name":"","cwd":"/tmp"}"#,
        )
        .unwrap();

        assert!(parse_session_file(&path).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_session_file_whitespace_only_name() {
        let dir = std::env::temp_dir().join("session_test_whitespace_name");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("12345.json");
        fs::write(
            &path,
            r#"{"pid":12345,"sessionId":"abc-def","name":"   ","cwd":"/tmp"}"#,
        )
        .unwrap();

        assert!(parse_session_file(&path).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_session_file_malformed_json() {
        let dir = std::env::temp_dir().join("session_test_malformed");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("12345.json");
        fs::write(&path, "not json at all").unwrap();

        assert!(parse_session_file(&path).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_session_file_nonexistent() {
        let path = std::env::temp_dir().join("session_test_nonexistent/99999.json");
        assert!(parse_session_file(&path).is_none());
    }

    #[test]
    fn session_alive_in_needs_matching_sid_and_live_pid() {
        let dir = std::env::temp_dir().join("session_alive_match");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        fs::write(
            dir.join("111.json"),
            r#"{"sessionId":"sid-a","pid":111,"procStart":"10"}"#,
        )
        .unwrap();

        // same sid + pid live (procStart matches) → alive
        assert!(session_alive_in(&dir, "sid-a", |pid, ps| pid == 111 && ps == "10"));
        // owner dead → not alive
        assert!(!session_alive_in(&dir, "sid-a", |_, _| false));
        // unknown sid (codex/opencode, or no file) → not alive
        assert!(!session_alive_in(&dir, "sid-b", |_, _| true));
        // missing dir → not alive
        assert!(!session_alive_in(
            std::path::Path::new("/nonexistent-session-dir"),
            "sid-a",
            |_, _| true
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_session_liveness_requires_all_fields() {
        let dir = std::env::temp_dir().join("session_alive_parse");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        fs::write(
            dir.join("1.json"),
            r#"{"sessionId":"s","pid":1,"procStart":"9"}"#,
        )
        .unwrap();
        fs::write(dir.join("2.json"), r#"{"sessionId":"s","procStart":"9"}"#).unwrap();
        fs::write(
            dir.join("3.json"),
            r#"{"sessionId":"","pid":3,"procStart":"9"}"#,
        )
        .unwrap();

        assert_eq!(
            parse_session_liveness(&dir.join("1.json")),
            Some(("s".into(), 1, "9".into()))
        );
        assert_eq!(parse_session_liveness(&dir.join("2.json")), None);
        assert_eq!(parse_session_liveness(&dir.join("3.json")), None);
        let _ = fs::remove_dir_all(&dir);
    }
}
