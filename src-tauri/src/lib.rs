// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
pub mod commands;
pub mod collector;
pub mod logging;
pub mod provider;
pub mod state;
pub mod store;

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tauri::{Emitter, Manager};

use crate::state::service::Aggregator;
use crate::store::Store;

/// 岛窗口标签（tauri.conf.json 中定义）
const ISLAND: &str = "island";

// ===== 贴边自动隐藏（追加需求：自由拖拽 + 贴边隐藏，默认开启） =====

/// 岛收缩态固定高度（逻辑像素）；宽度按屏幕自适应，见 island_width()
const ISLAND_H: i32 = 48;
/// 岛展开面板高度（逻辑像素）
const ISLAND_EXPANDED_H: i32 = 520;
/// 自适应宽度：显示器逻辑宽 × 比例，夹取 [MIN， MAX]（小屏保底、大屏封顶）。
/// 比例取 30%：介于三分律（1/3）与黄金分割小段（0.382）之间、主流悬浮组件
/// 25%~35% 区间的中值（NN/g、Figma 设计参考；所有者笔记本实测 +1/5 手感吻合）
const ISLAND_W_RATIO: f64 = 0.30;
const ISLAND_W_MIN: i32 = 380;
const ISLAND_W_MAX: i32 = 800;
/// 顶部贴边隐藏的露出高度（≈胶囊 48 的 1/5 略加余量：胶囊底部的独立信息条）
const PEEK_TOP_H: i32 = 14;
/// 左右贴边隐藏的伸出宽度（半圆 D 形标签）
const PEEK_SIDE_W: i32 = 20;
/// 贴靠判定阈值（逻辑像素）：拖放位置距屏幕边小于该值即吸附到该边
const SNAP_THRESHOLD: i32 = 24;
/// 拖拽防抖：Moved 事件静默该时长且左键已释放，才认定"拖放完成"并评估贴靠
const DRAG_QUIET_MS: u128 = 180;

/// 岛自适应宽度：显示器逻辑宽 × 30%，夹取 [380， 800]。
/// 1366→410 / 1440→432 / 1920→576 / 2560→768 / 3840→800；Rust 贴边几何与前端渲染共用
fn island_width(mon_logical_w: i32) -> i32 {
    ((mon_logical_w as f64 * ISLAND_W_RATIO).round() as i32)
        .clamp(ISLAND_W_MIN, ISLAND_W_MAX)
}

/// 岛的运动/贴边状态（内存态，setup 时创建并全局共享；位置与开关持久化在 app_settings）
#[derive(Default)]
struct IslandMotion {
    /// 拖拽中的待评估位置（事件时间戳 + 坐标），看护线程防抖后消费
    pending: Option<(std::time::Instant, i32, i32)>,
    /// 程序化滑动（吸附/隐藏/显示）的目标落点：用于识别并消费动画自己产生的 Moved 事件
    programmed: Option<(i32, i32)>,
    /// 滑动动画进行中：开始时刻 + 落点。Moved 事件消费落点即结束；
    /// 落点事件意外丢失时按超时自复位，防"动画标记永久卡死"（审查 3.7:
    /// 由独立的兜底线程改为时间戳判定，少一个短命线程）
    animating: Option<(std::time::Instant, (i32, i32))>,
    /// 当前贴靠边："none" | "top" | "left" | "right"
    edge: String,
    /// 是否处于滑出隐藏态
    hidden: bool,
}

/// 动画标记超时：超过该时长仍未等到落点 Moved 事件则自复位（正常动画约 130~200ms）
const ANIM_TIMEOUT_MS: u128 = 400;

/// 滑动动画代数：每次新滑动/用户拖拽都递增，使旧动画线程自行退出
static SLIDE_GEN: AtomicU64 = AtomicU64::new(0);

/// 点击会话卡片 → 激活对应终端/IDE 窗口（T10，窗口级定位）
#[tauri::command]
fn focus_session(session_id: String, store: tauri::State<'_, Arc<Store>>) -> bool {
    let Some((agent, project_dir)) = store.get_session_meta(&session_id) else {
        return false;
    };
    commands::find_session_window(&agent, project_dir.as_deref())
        .map(commands::activate_window)
        .unwrap_or(false)
}

// ===== 设置页 commands（T11） =====

/// 允许前端写入的设置键白名单（审查 2.1.3）：防止任意键写入
/// （如覆盖 hook_events_offset/island_pos 等内部状态键）
const SETTING_KEYS_ALLOW: &[&str] = &[
    "glm_base",
    "glm_token",
    "threshold_warn",
    "threshold_danger",
    "cleanup_days",
    "island_autohide",
    "hover_expand",
    "agents_enabled",
    "agent_colors",
    "theme",
];

/// 读取全部设置。
/// glm_token 原样随设置下发（2026-09-17 所有者要求 API Key 回显输入框，
/// 推翻原审查 2.1.2"敏感值不下发前端"的决策；仅下发到本机自身窗口）
#[tauri::command]
fn get_settings(store: tauri::State<'_, Arc<Store>>) -> std::collections::HashMap<String, String> {
    store.all_settings()
}

/// 写单条设置（白名单外的键拒绝并报错，前端会显示"保存失败"）
#[tauri::command]
fn set_setting(key: String, value: String, store: tauri::State<'_, Arc<Store>>) -> Result<(), String> {
    if !SETTING_KEYS_ALLOW.contains(&key.as_str()) {
        log::warn!("拒绝写入未登记的设置键：{key}");
        return Err(format!("不允许写入设置键：{key}"));
    }
    store.set_setting(&key, &value);
    Ok(())
}

/// hooks 安装状态（检查 settings.json 中是否存在自家注入条目）
#[tauri::command]
fn hooks_status() -> bool {
    collector::claude_code::hooks_installed()
}

/// 安装 hooks（增强档：精确状态）。
/// async 标记：文件 IO 移出主线程，不阻塞事件循环（Tauri 语义，审查 2.2.1）
#[tauri::command(async)]
fn install_hooks() -> Result<usize, String> {
    collector::claude_code::install_hooks().map_err(|e| e.to_string())
}

/// 卸载 hooks（还原 settings.json）；async 标记理由同上
#[tauri::command(async)]
fn uninstall_hooks() -> Result<usize, String> {
    collector::claude_code::uninstall_hooks().map_err(|e| e.to_string())
}

/// 开机自启状态
#[tauri::command]
fn autostart_get(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch()
        .is_enabled()
        .map_err(|e| e.to_string())
}

/// 设置开机自启（默认关，红线⑤）
#[tauri::command]
fn autostart_set(app: tauri::AppHandle, enable: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let al = app.autolaunch();
    if enable {
        al.enable().map_err(|e| e.to_string())
    } else {
        al.disable().map_err(|e| e.to_string())
    }
}

// ===== 报表 commands（M1-1） =====
// 报表聚合是重查询（审查 2.2.1：Tauri 同步 command 在主线程执行，"全部"范围
// 大数据量时会冻结包括岛在内的全部窗口）——统一走 spawn_blocking 挪到线程池

/// 报表查询公共壳：State 不能跨线程移动，先克隆 Arc 再进阻塞线程池
async fn run_report<T>(
    store: tauri::State<'_, Arc<Store>>,
    query: impl FnOnce(&Store) -> Vec<T> + Send + 'static,
) -> Result<Vec<T>, String>
where
    T: Send + 'static,
{
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || query(&store))
        .await
        .map_err(|e| format!("报表查询任务失败：{e}"))
}

/// 报表：按日聚合用量（days<=0 表示全部历史，下同）
#[tauri::command]
async fn report_daily(
    days: i64,
    store: tauri::State<'_, Arc<Store>>,
) -> Result<Vec<store::DayUsage>, String> {
    run_report(store, move |s| s.report_daily(days)).await
}

/// 报表：按模型聚合
#[tauri::command]
async fn report_by_model(
    days: i64,
    store: tauri::State<'_, Arc<Store>>,
) -> Result<Vec<store::SliceUsage>, String> {
    run_report(store, move |s| s.report_by_model(days)).await
}

/// 报表：按供应商聚合
#[tauri::command]
async fn report_by_provider(
    days: i64,
    store: tauri::State<'_, Arc<Store>>,
) -> Result<Vec<store::SliceUsage>, String> {
    run_report(store, move |s| s.report_by_provider(days)).await
}

/// 报表：星期×小时热力图
#[tauri::command]
async fn report_heatmap(
    days: i64,
    store: tauri::State<'_, Arc<Store>>,
) -> Result<Vec<store::HeatCell>, String> {
    run_report(store, move |s| s.report_heatmap(days)).await
}

// ===== 贴边自动隐藏 commands（追加需求） =====

/// 岛自适应尺寸（前端挂载时获取，Rust 贴边几何与前端渲染共用同一公式）
#[derive(serde::Serialize)]
struct IslandMetrics {
    width: i32,
    collapsed_h: i32,
    expanded_h: i32,
}

/// 查询岛自适应尺寸（基于岛窗口当前所在显示器的逻辑宽度）
#[tauri::command]
fn island_metrics(win: tauri::WebviewWindow) -> Option<IslandMetrics> {
    let mon = win.current_monitor().ok().flatten()?;
    let ml = monitor_logical(&mon);
    Some(IslandMetrics {
        width: island_width(ml.2),
        collapsed_h: ISLAND_H,
        expanded_h: ISLAND_EXPANDED_H,
    })
}

/// 鼠标移入贴边岛 → 滑入显示（show=true）/移出 → 滑出隐藏（show=false）。
/// 未贴边（edge=none）或关闭自动隐藏时为无害 no-op
#[tauri::command]
fn island_peek(
    show: bool,
    app: tauri::AppHandle,
    store: tauri::State<'_, Arc<Store>>,
    motion: tauri::State<'_, Arc<Mutex<IslandMotion>>>,
) {
    let (edge, hidden) = {
        let m = motion.lock().unwrap();
        (m.edge.clone(), m.hidden)
    };
    if edge == "none" {
        return;
    }
    if show && hidden {
        peek_apply(&app, store.as_ref(), &motion, false);
    } else if !show && !hidden && autohide_enabled(store.as_ref()) {
        peek_apply(&app, store.as_ref(), &motion, true);
    }
}

/// 贴边相关设置变更后的状态修正（双向对称）：
/// - 关闭自动隐藏时岛正处于隐藏态 → 滑回停靠位显示
/// - 开启自动隐藏时岛正停靠可见 → 立即滑出隐藏
#[tauri::command]
fn island_refresh(
    app: tauri::AppHandle,
    store: tauri::State<'_, Arc<Store>>,
    motion: tauri::State<'_, Arc<Mutex<IslandMotion>>>,
) {
    let (edge, hidden) = {
        let m = motion.lock().unwrap();
        (m.edge.clone(), m.hidden)
    };
    if edge != "none" {
        let autohide = autohide_enabled(store.as_ref());
        if hidden && !autohide {
            peek_apply(&app, store.as_ref(), &motion, false);
        } else if !hidden && autohide {
            peek_apply(&app, store.as_ref(), &motion, true);
        }
    }
}

/// 查询岛当前贴边/隐藏状态（前端挂载时主动拉取一次：启动恢复发生在 setup 阶段，
/// 早于前端事件监听建立，事件推送会漏掉首帧，导致重启后的隐藏态渲染成完整胶囊）
#[tauri::command]
fn island_dock_state(motion: tauri::State<'_, Arc<Mutex<IslandMotion>>>) -> serde_json::Value {
    let m = motion.lock().unwrap();
    serde_json::json!({ "edge": m.edge, "hidden": m.hidden })
}

/// 用户按下岛（拖拽开始）：取消在播滑动动画与待评估位置，
/// 避免拖拽循环和滑动动画互相抢窗口（程序化 set_position 会打断系统拖拽）
#[tauri::command]
fn island_drag_start(motion: tauri::State<'_, Arc<Mutex<IslandMotion>>>) {
    SLIDE_GEN.fetch_add(1, Ordering::Relaxed);
    let mut m = motion.lock().unwrap();
    m.animating = None;
    m.programmed = None;
    m.pending = None;
}

/// 贴边自动隐藏开关（设置项 island_autohide，缺省=开）
fn autohide_enabled(store: &Store) -> bool {
    store
        .get_setting("island_autohide")
        .map(|v| v != "0")
        .unwrap_or(true)
}

/// 解析 island_pos 设置（"x,y" 物理坐标）
fn saved_pos(store: &Store) -> Option<(i32, i32)> {
    store.get_setting("island_pos").and_then(|s| {
        s.split_once(',')
            .and_then(|(a, b)| match (a.trim().parse::<i32>(), b.trim().parse::<i32>()) {
                (Ok(x), Ok(y)) => Some((x, y)),
                _ => None,
            })
    })
}

/// 物理坐标 → 逻辑坐标（按显示器缩放换算）
fn phys_to_logical(v: (i32, i32), scale: f64) -> (i32, i32) {
    (
        (v.0 as f64 / scale).round() as i32,
        (v.1 as f64 / scale).round() as i32,
    )
}

/// 逻辑坐标 → 物理坐标
fn logical_to_phys(v: (i32, i32), scale: f64) -> (i32, i32) {
    (
        (v.0 as f64 * scale).round() as i32,
        (v.1 as f64 * scale).round() as i32,
    )
}

/// 显示器矩形（逻辑坐标）：x， y， w， h
fn monitor_logical(mon: &tauri::Monitor) -> (i32, i32, i32, i32) {
    let s = mon.scale_factor();
    let mp = mon.position();
    (
        (mp.x as f64 / s).round() as i32,
        (mp.y as f64 / s).round() as i32,
        (mon.size().width as f64 / s).round() as i32,
        (mon.size().height as f64 / s).round() as i32,
    )
}

/// 判定拖放位置应吸附的屏幕边（优先级：上 > 左 > 右；越界超出阈值视为自由位置）。
/// mon = (x， y， w， h) 显示器矩形；独立函数便于几何单测
fn detect_edge(
    pos: (i32, i32),
    mon: (i32, i32, i32, i32),
    size: (i32, i32),
    threshold: i32,
) -> &'static str {
    let (mx, my, mw, _) = mon;
    let (x, y) = pos;
    if y - my <= threshold {
        "top"
    } else if x - mx <= threshold {
        "left"
    } else if mx + mw - (x + size.0) <= threshold {
        "right"
    } else {
        "none"
    }
}

/// 隐藏位坐标：按贴靠边把窗口滑出屏幕，仅留露出常量对应的信息条/半圆标签
/// （显示器以纯数值传入：mon_pos=原点，mon_w=宽度；独立函数便于几何单测）
fn hidden_pos(
    mon_pos: (i32, i32),
    mon_w: i32,
    edge: &str,
    docked: (i32, i32),
    island_w: i32,
) -> (i32, i32) {
    match edge {
        "top" => (docked.0, mon_pos.1 - (ISLAND_H - PEEK_TOP_H)),
        "left" => (mon_pos.0 - (island_w - PEEK_SIDE_W), docked.1),
        "right" => (mon_pos.0 + mon_w - PEEK_SIDE_W, docked.1),
        _ => docked,
    }
}

/// 程序化滑动窗口到位（to 为逻辑坐标；steps=1 即瞬时跳变；8~10 步约 130~200ms 动效）。
/// 动画期间产生的 Moved 事件由 IslandMotion.animating 屏蔽，落点由 programmed 消费，
/// 防止"移动 → Moved → 再评估 → 再移动"的自触发循环；落点事件意外丢失时
/// 由 Moved 处理器按超时自复位（审查 3.7：去掉独立兜底线程）
fn slide_to(
    win: &tauri::WebviewWindow,
    to: (i32, i32),
    scale: f64,
    motion: &Arc<Mutex<IslandMotion>>,
    steps: i32,
) {
    let Ok(from_phys) = win.outer_position() else {
        return;
    };
    let from = phys_to_logical((from_phys.x, from_phys.y), scale);
    if from == to {
        return;
    }
    let to_phys = logical_to_phys(to, scale);
    let gen = SLIDE_GEN.fetch_add(1, Ordering::Relaxed) + 1;
    {
        let mut m = motion.lock().unwrap();
        m.animating = Some((std::time::Instant::now(), to_phys));
        m.programmed = Some(to_phys);
    }
    let win = win.clone();
    std::thread::spawn(move || {
        let steps = steps.max(1);
        for i in 1..=steps {
            if SLIDE_GEN.load(Ordering::Relaxed) != gen {
                return; // 被更新的滑动/用户拖拽取代
            }
            let t = i as f64 / steps as f64;
            let lx = from.0 + ((to.0 - from.0) as f64 * t).round() as i32;
            let ly = from.1 + ((to.1 - from.1) as f64 * t).round() as i32;
            let phys = logical_to_phys((lx, ly), scale);
            let _ = win.set_position(tauri::PhysicalPosition::new(phys.0, phys.1));
            std::thread::sleep(Duration::from_millis(16));
        }
    });
}

/// 执行隐藏/显示：滑向隐藏位或停靠位，同步内存状态并向岛窗口广播 island-dock 事件
fn peek_apply(
    app: &tauri::AppHandle,
    store: &Store,
    motion: &Arc<Mutex<IslandMotion>>,
    hide: bool,
) {
    let Some(win) = app.get_webview_window(ISLAND) else {
        return;
    };
    let (edge, docked) = {
        let m = motion.lock().unwrap();
        (m.edge.clone(), saved_pos(store))
    };
    if edge == "none" || docked.is_none() {
        return;
    }
    let docked = docked.unwrap();
    let Ok(Some(mon)) = win.current_monitor() else {
        return;
    };
    let ml = monitor_logical(&mon);
    let width = island_width(ml.2);
    // 隐藏/显示前强制收回收缩态尺寸：面板展开时贴边隐藏，若按 520 高度渲染，
    // 锚定窗口底部的标签会出现在面板下方而非屏幕边缘
    let _ = win.set_size(tauri::LogicalSize::new(width, ISLAND_H));
    let to = if hide {
        hidden_pos((ml.0, ml.1), ml.2, &edge, docked, width)
    } else {
        docked
    };
    {
        let mut m = motion.lock().unwrap();
        m.hidden = hide;
    }
    let scale = mon.scale_factor();
    slide_to(&win, to, scale, motion, 8);
    let m = motion.lock().unwrap();
    let _ = app.emit_to(
        ISLAND,
        "island-dock",
        serde_json::json!({"edge": m.edge, "hidden": m.hidden}),
    );
}

/// 拖放后的贴靠评估：吸附到最近的屏幕边，按设置决定是否滑出隐藏；自由位置则原样记忆。
/// 全程使用逻辑坐标（物理事件坐标按显示器缩放换算），与窗口逻辑尺寸/前端 CSS 一致
fn apply_snap(
    app: &tauri::AppHandle,
    store: &Store,
    motion: &Arc<Mutex<IslandMotion>>,
    pos: (i32, i32),
) {
    let Some(win) = app.get_webview_window(ISLAND) else {
        return;
    };
    let Ok(Some(mon)) = win.current_monitor() else {
        return;
    };
    let scale = mon.scale_factor();
    let ml = monitor_logical(&mon);
    let pos_l = phys_to_logical(pos, scale);
    let width = island_width(ml.2);
    let size = (width, ISLAND_H);

    let edge = detect_edge(pos_l, ml, size, SNAP_THRESHOLD);
    let (docked, hide) = match edge {
        "top" => (
            (
                pos_l.0.clamp(ml.0, ml.0 + ml.2 - size.0),
                ml.1,
            ),
            autohide_enabled(store),
        ),
        "left" => (
            (ml.0, pos_l.1.clamp(ml.1, ml.1 + ml.3 - size.1)),
            autohide_enabled(store),
        ),
        "right" => (
            (
                ml.0 + ml.2 - size.0,
                pos_l.1.clamp(ml.1, ml.1 + ml.3 - size.1),
            ),
            autohide_enabled(store),
        ),
        _ => (pos_l, false),
    };
    {
        let mut m = motion.lock().unwrap();
        m.edge = edge.to_string();
        m.hidden = hide;
    }
    store.set_setting("island_pos", &format!("{},{}", docked.0, docked.1));
    store.set_setting("island_edge", edge);
    // 吸附隐藏前强制收回收缩态尺寸（拖拽时面板可能仍展开，理由同 peek_apply）
    if hide {
        let _ = win.set_size(tauri::LogicalSize::new(width, ISLAND_H));
    }
    let to = if hide {
        hidden_pos((ml.0, ml.1), ml.2, edge, docked, width)
    } else {
        docked
    };
    slide_to(&win, to, scale, motion, 8);
    let m = motion.lock().unwrap();
    let _ = app.emit_to(
        ISLAND,
        "island-dock",
        serde_json::json!({"edge": m.edge, "hidden": m.hidden}),
    );
}

/// 左键是否按住（拖拽进行中不评估贴靠，防止拖到半路被吸附走）
fn lbutton_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    // SAFETY:GetAsyncKeyState 仅查询系统全局按键状态，无指针/生命周期风险；
    // 返回值短时置位语义与本用途（按住检测）兼容
    unsafe { GetAsyncKeyState(VK_LBUTTON.0.into()) as u16 & 0x8000 != 0 }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // （2026-09-17 审查 3.1）tauri-plugin-opener 全项目零引用，已随依赖移除
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            focus_session,
            get_settings,
            set_setting,
            hooks_status,
            install_hooks,
            uninstall_hooks,
            autostart_get,
            autostart_set,
            report_daily,
            report_by_model,
            report_by_provider,
            report_heatmap,
            island_peek,
            island_refresh,
            island_dock_state,
            island_drag_start,
            island_metrics
        ])
        .setup(|app| {
            // 自库：%APPDATA%\com.agenttrackerisland.app\agenttrackerisland.db
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let store = Arc::new(Store::open(&dir.join("agenttrackerisland.db"))?);
            app.manage(store.clone());

            // 日志与 panic 钩子（审查 1.1）：一切降级/异常必须留痕可查
            logging::init(&dir.join("logs"));
            logging::install_panic_hook();

            let win = app
                .get_webview_window(ISLAND)
                .ok_or_else(|| anyhow::anyhow!("island 窗口未在配置中定义"))?;

            // 毛玻璃：❌不使用 window-vibrancy——Acrylic 是窗口级效果，会把整个
            // 矩形窗口染成磨砂灰，破坏胶囊形态；岛的正确做法=窗口全透明+CSS 自绘
            // 背景（见 App.css）。依赖保留备注：M1 若做全宽岛形态可再启用。

            // 贴边/拖拽运动状态（setup 内创建，commands 与看护线程经 manage 共享）；
            // 必须先于 position_island 创建，启动定位要经它防误判
            let motion = Arc::new(Mutex::new(IslandMotion {
                edge: store
                    .get_setting("island_edge")
                    .unwrap_or_else(|| "none".into()),
                ..Default::default()
            }));
            app.manage(motion.clone());

            // 定位：记忆坐标优先，否则顶部居中
            position_island(&win, store.as_ref(), &motion);

            // 窗口移动事件：程序化滑动的落点被消费忽略；用户拖拽则记录待评估位置
            {
                let motion2 = motion.clone();
                win.on_window_event(move |ev| {
                    let tauri::WindowEvent::Moved(pos) = ev else {
                        return;
                    };
                    let mut m = motion2.lock().unwrap();
                    if let Some((since, target)) = m.animating {
                        // 动画产生的移动：仅当到达落点时消费并结算动画
                        if (pos.x, pos.y) == target {
                            m.animating = None;
                            m.programmed = None;
                            drop(m);
                            SLIDE_GEN.fetch_add(1, Ordering::Relaxed);
                        } else if since.elapsed().as_millis() > ANIM_TIMEOUT_MS {
                            // 落点事件意外丢失：超时自复位，防止动画标记永久卡死
                            m.animating = None;
                            m.programmed = None;
                        }
                        return;
                    }
                    m.programmed = None;
                    // 用户拖拽：打断在播动画，交给看护线程防抖评估
                    drop(m);
                    SLIDE_GEN.fetch_add(1, Ordering::Relaxed);
                    motion2.lock().unwrap().pending =
                        Some((std::time::Instant::now(), pos.x, pos.y));
                });
            }

            // 贴靠看护线程：位置静默且左键释放（拖放完成）后评估吸附/隐藏
            {
                let motion2 = motion.clone();
                let app2 = app.handle().clone();
                let store2 = store.clone();
                std::thread::spawn(move || loop {
                    std::thread::sleep(Duration::from_millis(90));
                    let fire = {
                        let mut m = motion2.lock().unwrap();
                        match m.pending.take() {
                            Some((at, x, y))
                                if at.elapsed().as_millis() >= DRAG_QUIET_MS
                                    && !lbutton_down() =>
                            {
                                Some((x, y))
                            }
                            Some(other) => {
                                m.pending = Some(other);
                                None
                            }
                            None => None,
                        }
                    };
                    if let Some((x, y)) = fire {
                        apply_snap(&app2, &store2, &motion2, (x, y));
                    }
                });
            }

            // 启动恢复：上次贴边 + 自动隐藏开启 → 瞬时滑出（仅露边）；其余按记忆坐标可见
            {
                let edge = motion.lock().unwrap().edge.clone();
                if edge != "none" && autohide_enabled(store.as_ref()) {
                    if let (Some(mon), Some(docked)) = (
                        win.current_monitor().ok().flatten(),
                        saved_pos(store.as_ref()),
                    ) {
                        let ml = monitor_logical(&mon);
                        slide_to(
                            &win,
                            hidden_pos((ml.0, ml.1), ml.2, &edge, docked, island_width(ml.2)),
                            mon.scale_factor(),
                            &motion,
                            1,
                        );
                        // 同步内存隐藏态：否则 island_peek 会误判"未隐藏"，悬停滑入失效
                        motion.lock().unwrap().hidden = true;
                    }
                }
            }

            // 设置/报表窗口：关闭即隐藏（而非销毁），保证托盘可反复唤起
            for label in ["settings", "report"] {
                if let Some(w) = app.get_webview_window(label) {
                    let w2 = w.clone();
                    w.on_window_event(move |ev| {
                        if let tauri::WindowEvent::CloseRequested { api, .. } = ev {
                            api.prevent_close();
                            let _ = w2.hide();
                        }
                    });
                }
            }

            build_tray(app)?;

            // 数据清理：按设置周期启动时执行一次（0=永不；未设置默认保留 1 年，
            // 审查 3.8：status_events 每工具调用一条，永不清理会让库无限膨胀）
            let cleanup_days = store
                .get_setting("cleanup_days")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(365);
            if cleanup_days > 0 {
                let cutoff = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64 - cleanup_days * 86_400_000)
                    .unwrap_or(0);
                let removed = store.cleanup_older_than(cutoff);
                if removed > 0 {
                    log::info!("启动清理：按保留 {cleanup_days} 天删除 {removed} 条过期数据");
                }
            }

            spawn_aggregator(app.handle().clone(), store);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 岛定位：有记忆坐标用之（逻辑坐标，夹取进显示器防丢失）；否则按主显示器顶部居中。
/// 必须经 motion 通道移动（slide_to 瞬时档）：否则启动定位产生的 Moved 事件
/// 会被看护线程误判为"拖放"，把距顶边仅 6px 的岛自动吸附隐藏
fn position_island(
    win: &tauri::WebviewWindow,
    store: &Store,
    motion: &Arc<Mutex<IslandMotion>>,
) {
    let Ok(Some(mon)) = win.current_monitor() else {
        return;
    };
    let ml = monitor_logical(&mon);
    let width = island_width(ml.2);
    let pos = match saved_pos(store) {
        Some(p) => (
            p.0.clamp(ml.0, ml.0 + ml.2 - width),
            p.1.clamp(ml.1, ml.1 + ml.3 - ISLAND_H),
        ),
        None => (ml.0 + (ml.2 - width) / 2, ml.1 + 6),
    };
    slide_to(win, pos, mon.scale_factor(), motion, 1);
}

/// 系统托盘：常驻核心；菜单=显示/隐藏 + 设置 + 退出
fn build_tray(app: &tauri::App) -> anyhow::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;

    let toggle = MenuItem::with_id(app, "toggle", "显示 / 隐藏灵动岛", true, None::<&str>)?;
    let report = MenuItem::with_id(app, "report", "报表…", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &report, &settings, &quit])?;
    TrayIconBuilder::with_id("at-tray")
        .tooltip("AgentTrackerIsland")
        .icon(app.default_window_icon().expect("应用图标").clone())
        .menu(&menu)
        .on_menu_event(|app, ev| match ev.id.as_ref() {
            "quit" => app.exit(0),
            "toggle" => {
                if let Some(w) = app.get_webview_window(ISLAND) {
                    if w.is_visible().unwrap_or(false) {
                        let _ = w.hide();
                    } else {
                        let _ = w.show();
                    }
                }
            }
            "report" => {
                if let Some(w) = app.get_webview_window("report") {
                    let _ = (w.show(), w.set_focus());
                }
            }
            "settings" => {
                if let Some(w) = app.get_webview_window("settings") {
                    let _ = (w.show(), w.set_focus());
                }
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}

/// 后台聚合线程：10s tick → 快照广播给前端（02-DESIGN §2.3 调度）。
/// tick 全程 catch_unwind（审查 1.1）：单轮 panic 不允许杀死线程造成岛永久
/// 静默冻结——panic 已由全局钩子落盘，线程降级续跑，快照带 degraded 标志
fn spawn_aggregator(app: tauri::AppHandle, store: Arc<Store>) {
    std::thread::spawn(move || {
        let mut agg = Aggregator::new(store);
        loop {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| agg.tick()));
            match result {
                Ok(snap) => {
                    // 前端未监听时 emit 也只是无接收者，不报错
                    let _ = app.emit("island-snapshot", &snap);
                }
                Err(payload) => {
                    log::error!(
                        "聚合 tick panic（本轮无快照，线程续跑）：{}",
                        logging::panic_payload_str(payload)
                    );
                }
            }
            std::thread::sleep(Duration::from_secs(10));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 贴边几何判定：阈值内吸附，优先级 上 > 左 > 右
    #[test]
    fn test_detect_edge() {
        let mon = (0, 0, 1920, 1080); // 显示器矩形
        let size = (480, 48);
        // 屏幕中央：自由位置
        assert_eq!(detect_edge((720, 500), mon, size, SNAP_THRESHOLD), "none");
        // 顶部各处（含角落，顶部优先）
        assert_eq!(detect_edge((960, 10), mon, size, SNAP_THRESHOLD), "top");
        assert_eq!(detect_edge((5, 5), mon, size, SNAP_THRESHOLD), "top");
        // 左侧（不在顶部阈值内）
        assert_eq!(detect_edge((3, 500), mon, size, SNAP_THRESHOLD), "left");
        // 右侧：窗口右缘距屏幕右缘 20px <= 24
        assert_eq!(detect_edge((1920 - 460, 500), mon, size, SNAP_THRESHOLD), "right");
        // 恰好等于阈值：仍吸附（<=）
        assert_eq!(detect_edge((24, 500), mon, size, SNAP_THRESHOLD), "left");
        // 超出阈值一个像素：不吸附
        assert_eq!(detect_edge((25, 500), mon, size, SNAP_THRESHOLD), "none");
        // 顶部超出阈值：落空到左侧判断
        assert_eq!(detect_edge((960, 25), mon, size, SNAP_THRESHOLD), "none");
    }

    /// 隐藏位计算：三种贴靠边各露 PEEK_PX 在屏内
    #[test]
    fn test_hidden_pos() {
        let mon_pos = (0, 0);
        let mon_w = 1920;
        let width = island_width(mon_w); // 480
        // 顶部：上滑，底部露出 PEEK_TOP_H（独立信息条）
        assert_eq!(hidden_pos(mon_pos, mon_w, "top", (720, 0), width), (720, -(ISLAND_H - PEEK_TOP_H)));
        // 左侧：左滑，右侧露出 PEEK_SIDE_W（半圆标签）
        assert_eq!(hidden_pos(mon_pos, mon_w, "left", (0, 300), width), (-(width - PEEK_SIDE_W), 300));
        // 右侧：右滑，左缘留在 1920-PEEK_SIDE_W
        assert_eq!(hidden_pos(mon_pos, mon_w, "right", (1440, 300), width), (1920 - PEEK_SIDE_W, 300));
    }

    /// 自适应宽度：按屏幕逻辑宽 30% 夹取 [380， 800]
    #[test]
    fn test_island_width() {
        assert_eq!(island_width(1280), 384);
        assert_eq!(island_width(1366), 410); // 所有者笔记本：+1/5 手感校准
        assert_eq!(island_width(1440), 432);
        assert_eq!(island_width(1920), 576);
        assert_eq!(island_width(2560), 768);
        assert_eq!(island_width(3840), 800); // 大屏封顶
    }
}
