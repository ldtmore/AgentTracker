//! 窗口跳转(T10):点击会话卡片激活对应的终端/IDE 窗口(窗口级定位)。
//! 匹配策略(优先级从高到低):窗口标题含完整项目路径 > 含项目目录名 >
//! Agent 关键词(zcode 桌面窗口 / claude 终端);全部未命中返回 false。

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    SetForegroundWindow, ShowWindow, SW_RESTORE,
};

// 枚举结果的 thread_local 收集器(模块级单实例,回调与读取共用)
// 元组:(句柄, 标题, 所属进程 PID)
thread_local! {
    static FOUND: std::cell::RefCell<Vec<(isize, String, u32)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// 枚举当前全部可见顶层窗口,返回 (句柄, 标题, PID)
pub fn collect_visible_windows() -> Vec<(isize, String, u32)> {
    FOUND.with(|f| f.borrow_mut().clear());
    unsafe {
        // SAFETY:回调只写入 thread_local 向量;EnumWindows 同步完成
        let _ = EnumWindows(Some(enum_proc), LPARAM(0));
    }
    FOUND.with(|f| f.borrow().clone())
}

/// SAFETY:仅读写 thread_local 向量,不做其他不安全操作
unsafe extern "system" fn enum_proc(hwnd: HWND, _: LPARAM) -> BOOL {
    if !hwnd.0.is_null() && IsWindowVisible(hwnd).as_bool() {
        let mut buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut buf);
        if len > 0 {
            let title = String::from_utf16_lossy(&buf[..len as usize]);
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            FOUND.with(|f| f.borrow_mut().push((hwnd.0 as isize, title, pid)));
        }
    }
    BOOL(1) // 继续枚举
}

/// 按会话信息寻找最佳匹配窗口句柄
/// 匹配策略:标题含完整项目路径 > 含项目目录名 > 进程链匹配(跑 claude 的终端,
/// 解决 Windows Terminal 标题不含路径的问题)> Agent 关键词兜底
pub fn find_session_window(agent: &str, project_dir: Option<&str>) -> Option<isize> {
    let windows = collect_visible_windows();

    // ① 标题含完整项目路径(终端标题常显示当前目录)
    if let Some(dir) = project_dir {
        let target = dir.to_ascii_lowercase().replace('/', "\\");
        if !target.is_empty() {
            if let Some((h, _, _)) = windows
                .iter()
                .find(|(_, t, _)| t.to_ascii_lowercase().contains(&target))
            {
                return Some(*h);
            }
        }
        // ② 标题含项目目录末段(如 AgentTrackerIsland)
        if let Some(name) = dir.rsplit(['\\', '/']).next().filter(|s| !s.is_empty()) {
            let name = name.to_ascii_lowercase();
            if name.len() >= 3 {
                // 过短目录名(如 src)误匹配率高,跳过
                if let Some((h, _, _)) = windows
                    .iter()
                    .find(|(_, t, _)| t.to_ascii_lowercase().contains(&name))
                {
                    return Some(*h);
                }
            }
        }
    }

    // ③ 进程链匹配:找到命令行在跑 claude 的终端进程,沿父链爬到宿主窗口
    // (Windows Terminal 标签无路径信息,但 pwsh/node 是 WT 子进程,按 PID 反查窗口)
    if agent == "claude-code" {
        if let Some(h) = find_terminal_running_claude(&windows) {
            return Some(h);
        }
    }

    // ④ Agent 关键词兜底:zcode 桌面窗口 / claude 终端
    let keyword = if agent == "zcode" { "zcode" } else { "claude" };
    let hit = windows
        .iter()
        .find(|(_, t, _)| t.to_ascii_lowercase().contains(keyword))
        .map(|(h, _, _)| *h);
    if hit.is_none() {
        // 跳转未命中诊断(T10 调试线索,R14):dev 控制台可见;release 无控制台自然静默
        eprintln!("[focus] 未命中 agent={agent} project_dir={project_dir:?},当前可见窗口:");
        for (h, t, pid) in windows.iter().take(40) {
            let title: String = t.chars().take(60).collect();
            eprintln!("[focus]   hwnd={h} pid={pid} title={title}");
        }
    }
    hit
}

/// 在跑 claude 的终端进程 → 其(祖先)顶层窗口
fn find_terminal_running_claude(windows: &[(isize, String, u32)]) -> Option<isize> {
    use sysinfo::{ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    // 候选:shell/node 进程且命令行含 claude(排除自身与菜单脚本误报)
    let mut wanted_pids: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for (pid, proc) in sys.processes() {
        let name = proc.name().to_string_lossy().to_ascii_lowercase();
        let is_shell = name.contains("pwsh")
            || name.contains("powershell")
            || name.contains("node")
            || name.contains("cmd")
            || name.contains("claude");
        if !is_shell {
            continue;
        }
        let cmd = proc
            .cmd()
            .iter()
            .map(|a| a.to_string_lossy())
            .collect::<String>()
            .to_ascii_lowercase();
        if cmd.contains("claude")
            && !cmd.contains("agenttrackerisland")
            && !cmd.contains("claude-menu")
        {
            // 沿父进程链全部标记(直到 WT 宿主/无父)
            let mut cur = Some(*pid);
            while let Some(p) = cur {
                #[cfg(windows)]
                let as_u32 = p.as_u32();
                #[cfg(not(windows))]
                let as_u32 = u32::try_from(p.0).unwrap_or(0);
                if !wanted_pids.insert(as_u32) {
                    break; // 环,防御
                }
                cur = sys.process(p).and_then(|pr| pr.parent());
            }
        }
    }
    if wanted_pids.is_empty() {
        eprintln!("[focus] 进程链未找到在跑 claude 的终端进程(T10 调试,R14)");
        return None;
    }
    eprintln!("[focus] 在跑 claude 的候选进程 PID: {:?}", wanted_pids);
    // 窗口按 PID 命中(优先标题最长的,避免 "Default" 之类空壳)
    let mut hits: Vec<&(isize, String, u32)> = windows
        .iter()
        .filter(|(_, _, pid)| wanted_pids.contains(pid))
        .collect();
    hits.sort_by_key(|(_, t, _)| std::cmp::Reverse(t.len()));
    hits.first().map(|(h, _, _)| *h)
}

/// 激活窗口:还原(最小化时)+ 置前台
pub fn activate_window(handle: isize) -> bool {
    let hwnd = HWND(handle as *mut core::ffi::c_void);
    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE);
        SetForegroundWindow(hwnd).as_bool()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实桌面环境验证(手动:cargo test -- --ignored test_real_windows)
    #[test]
    #[ignore]
    fn test_real_windows() {
        let wins = collect_visible_windows();
        assert!(!wins.is_empty(), "桌面应有可见窗口");
        println!("可见窗口 {} 个,样例:", wins.len());
        for (h, t, pid) in wins.iter().filter(|(_, t, _)| !t.trim().is_empty()).take(5) {
            println!("  [{h}] pid={pid} {}", t.chars().take(50).collect::<String>());
        }
        // zcode 桌面在跑:关键词应命中
        assert!(find_session_window("zcode", None).is_some(), "应能找到 zcode 窗口");
        // claude-code:进程链匹配(本机有 claude 在 WT 中运行)
        if let Some(h) = find_session_window("claude-code", None) {
            println!("claude-code 进程链命中窗口: {h}");
        } else {
            println!("(当前无运行中的 claude 终端,进程链未命中属正常)");
        }
    }
}
