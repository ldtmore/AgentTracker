//! hooks 事件文件消费者:增量读取 hook-bridge 写入的事件文件。
//! 协议(hook-bridge.js 白名单字段,实测于 2026-09-16):
//!   每行 {"ts":毫秒,"hook":"Stop","session_id":"...","tool_name":?,"message":?}
//! 读取按字节偏移增量(文件 append-only),断电/重启后从上次偏移继续——红线③。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 一条 hook 状态事件(Serialize 供审计落库 status_events)
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct HookEvent {
    pub ts: i64,
    pub hook: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

/// 事件文件默认路径:%LOCALAPPDATA%\AgentTrackerIsland\events\claude-code.jsonl
pub fn events_file_path() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(local).join("AgentTrackerIsland").join("events").join("claude-code.jsonl"))
}

/// 增量读取:返回(新事件, 新偏移)。
/// 偏移语义:已消费的字节位置,只会推进到最后一个完整换行处——
/// 上次停在半行中间(offset 前一字符非 '\n')则先跳到下一个换行后再解析。
/// 文件不存在或行损坏时跳过坏行不阻塞。
pub fn read_events(path: &std::path::Path, offset: u64) -> (Vec<HookEvent>, u64) {
    let mut events = vec![];
    if !path.exists() {
        return (events, offset);
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return (events, offset); // 文件被占用等情况:本轮跳过
    };
    let bytes = content.as_bytes();
    let total = bytes.len() as u64;
    if total <= offset {
        return (events, offset); // 无新增
    }
    // 确定解析起点:offset 恰在行边界(前一字符是换行或 offset==0)则直接用;
    // 否则上次停在半行中间,跳到下一个换行之后
    let start_idx = if offset == 0 || bytes[(offset - 1) as usize] == b'\n' {
        offset as usize
    } else {
        match content[offset as usize..].find('\n') {
            Some(i) => offset as usize + i + 1,
            None => return (events, offset), // 半行且未写完:本轮不推进
        }
    };
    // 逐行解析,同时记录最后一个完整换行的位置作为新偏移
    let mut new_offset = offset;
    let mut cursor = start_idx as u64;
    for line in content[start_idx..].split_inclusive('\n') {
        if !line.ends_with('\n') {
            break; // 末尾半行:留给下一轮
        }
        cursor += line.len() as u64;
        new_offset = cursor;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(ev) = serde_json::from_str::<HookEvent>(trimmed) {
            events.push(ev);
        } // 坏行静默忽略
    }
    (events, new_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_events_incremental() {
        let dir = std::env::temp_dir().join(format!("at-t6-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("events.jsonl");

        // 第一次:两行
        std::fs::write(&f, concat!(
            r#"{"ts":1,"hook":"SessionStart","session_id":"s1"}"#, "\n",
            r#"{"ts":2,"hook":"UserPromptSubmit","session_id":"s1"}"#, "\n"
        )).unwrap();
        let (evs, off) = read_events(&f, 0);
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].hook, "SessionStart");
        assert!(off > 0);

        // 无新增:空
        let (evs2, off2) = read_events(&f, off);
        assert!(evs2.is_empty());
        assert_eq!(off2, off);

        // 追加一行(模拟半行竞态后再写全)
        std::fs::write(&f, format!(
            "{}{}",
            std::fs::read_to_string(&f).unwrap(),
            concat!(r#"{"ts":3,"hook":"Stop","session_id":"s1","message":"done"}"#, "\n")
        )).unwrap();
        let (evs3, _) = read_events(&f, off2);
        assert_eq!(evs3.len(), 1);
        assert_eq!(evs3[0].hook, "Stop");
        assert_eq!(evs3[0].message.as_deref(), Some("done"));

        // 文件不存在:空且偏移不变
        let (evs4, off4) = read_events(&dir.join("nope.jsonl"), 42);
        assert!(evs4.is_empty());
        assert_eq!(off4, 42);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
