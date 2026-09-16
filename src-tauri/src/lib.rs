// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
pub mod commands;
pub mod collector;
pub mod provider;
pub mod state;
pub mod store;

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tauri::{Emitter, Manager};

use crate::state::service::Aggregator;
use crate::store::Store;

/// 岛窗口标签(tauri.conf.json 中定义)
const ISLAND: &str = "island";

// ===== 贴边自动隐藏(追加需求:自由拖拽 + 贴边隐藏,默认开启) =====

/// 岛收缩态固定高度(逻辑像素);宽度按屏幕自适应,见 island_width()
const ISLAND_H: i32 = 48;
/// 岛展开面板高度(逻辑像素)
const ISLAND_EXPANDED_H: i32 = 520;
/// 自适应宽度:显示器逻辑宽 × 比例,夹取 [MIN, MAX](小屏保底、大屏封顶)。
/// 比例取 30%:介于三分律(1/3)与黄金分割小段(0.382)之间、主流悬浮组件
/// 25%~35% 区间的中值(NN/g、Figma 设计参考;所有者笔记本实测 +1/5 手感吻合)
const ISLAND_W_RATIO: f64 = 0.30;
const ISLAND_W_MIN: i32 = 380;
const ISLAND_W_MAX: i32 = 800;
/// 顶部贴边隐藏的露出高度(≈胶囊 48 的 1/5 略加余量:胶囊底部的独立信息条)
const PEEK_TOP_H: i32 = 14;
/// 左右贴边隐藏的伸出宽度(半圆 D 形标签)
const PEEK_SIDE_W: i32 = 20;
/// 贴靠判定阈值(逻辑像素):拖放位置距屏幕边小于该值即吸附到该边
const SNAP_THRESHOLD: i32 = 24;
/// 拖拽防抖:Moved 事件静默该时长且左键已释放,才认定"拖放完成"并评估贴靠
const DRAG_QUIET_MS: u128 = 180;

/// 岛自适应宽度:显示器逻辑宽 × 30%,夹取 [380, 800]。
/// 1366→410 / 1440→432 / 1920→576 / 2560→768 / 3840→800;Rust 贴边几何与前端渲染共用
fn island_width(mon_logical_w: i32) -> i32 {
    ((mon_logical_w as f64 * ISLAND_W_RATIO).round() as i32)
        .clamp(ISLAND_W_MIN, ISLAND_W_MAX)
}

/// 岛的运动/贴边状态(内存态,setup 时创建并全局共享;位置与开关持久化在 app_settings)
#[derive(Default)]
struct IslandMotion {
    /// 拖拽中的待评估位置(事件时间戳 + 坐标),看护线程防抖后消费
    pending: Option<(std::time::Instant, i32, i32)>,
    /// 程序化滑动(吸附/隐藏/显示)的目标落点:用于识别并消费动画自己产生的 Moved 事件
    programmed: Option<(i32, i32)>,
    /// 滑动动画进行中(期间产生的 Moved 事件全部忽略,防止自触发循环)
    animating: bool,
    /// 当前贴靠边:"none" | "top" | "left" | "right"
    edge: String,
    /// 是否处于滑出隐藏态
    hidden: bool,
}

/// 滑动动画代数:每次新滑动/用户拖拽都递增,使旧动画线程自行退出
static SLIDE_GEN: AtomicU64 = AtomicU64::new(0);

/// 点击会话卡片 → 激活对应终端/IDE 窗口(T10,窗口级定位)
#[tauri::command]
fn focus_session(session_id: String, store: tauri::State<'_, Arc<Store>>) -> bool {
    let Some((agent, project_dir)) = store.get_session_meta(&session_id) else {
        return false;
    };
    commands::find_session_window(&agent, project_dir.as_deref())
        .map(commands::activate_window)
        .unwrap_or(false)
}

// ===== 设置页 commands(T11) =====

/// 读取全部设置(键值对;key 类敏感值原样返回——本地单用户工具,无多用户泄露面)
#[tauri::command]
fn get_settings(store: tauri::State<'_, Arc<Store>>) -> std::collections::HashMap<String, String> {
    store.all_settings()
}

/// 写单条设置
#[tauri::command]
fn set_setting(key: String, value: String, store: tauri::State<'_, Arc<Store>>) {
    store.set_setting(&key, &value);
}

/// hooks 安装状态(检查 settings.json 中是否存在自家注入条目)
#[tauri::command]
fn hooks_status() -> bool {
    collector::claude_code::hooks_installed()
}

/// 安装 hooks(增强档:精确状态)
#[tauri::command]
fn install_hooks() -> Result<usize, String> {
    collector::claude_code::install_hooks().map_err(|e| e.to_string())
}

/// 卸载 hooks(还原 settings.json)
#[tauri::command]
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

/// 设置开机自启(默认关,红线⑤)
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

// ===== 报表 commands(M1-1) =====

/// 报表:按日聚合用量(days<=0 表示全部历史,下同)
#[tauri::command]
fn report_daily(days: i64, store: tauri::State<'_, Arc<Store>>) -> Vec<store::DayUsage> {
    store.report_daily(days)
}

/// 报表:按模型聚合
#[tauri::command]
fn report_by_model(days: i64, store: tauri::State<'_, Arc<Store>>) -> Vec<store::SliceUsage> {
    store.report_by_model(days)
}

/// 报表:按供应商聚合
#[tauri::command]
fn report_by_provider(days: i64, store: tauri::State<'_, Arc<Store>>) -> Vec<store::SliceUsage> {
    store.report_by_provider(days)
}

/// 报表:星期×小时热力图
#[tauri::command]
fn report_heatmap(days: i64, store: tauri::State<'_, Arc<Store>>) -> Vec<store::HeatCell> {
    store.report_heatmap(days)
}

// ===== 贴边自动隐藏 commands(追加需求) =====

/// 岛自适应尺寸(前端挂载时获取,Rust 贴边几何与前端渲染共用同一公式)
#[derive(serde::Serialize)]
struct IslandMetrics {
    width: i32,
    collapsed_h: i32,
    expanded_h: i32,
}

/// 查询岛自适应尺寸(基于岛窗口当前所在显示器的逻辑宽度)
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

/// 鼠标移入贴边岛 → 滑入显示(show=true)/移出 → 滑出隐藏(show=false)。
/// 未贴边(edge=none)或关闭自动隐藏时为无害 no-op
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

/// 贴边相关设置变更后的状态修正:如关闭自动隐藏时岛正处于隐藏态 → 滑回停靠位显示
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
    if edge != "none" && hidden && !autohide_enabled(store.as_ref()) {
        peek_apply(&app, store.as_ref(), &motion, false);
    }
}

/// 用户按下岛(拖拽开始):取消在播滑动动画与待评估位置,
/// 避免拖拽循环和滑动动画互相抢窗口(程序化 set_position 会打断系统拖拽)
#[tauri::command]
fn island_drag_start(motion: tauri::State<'_, Arc<Mutex<IslandMotion>>>) {
    SLIDE_GEN.fetch_add(1, Ordering::Relaxed);
    let mut m = motion.lock().unwrap();
    m.animating = false;
    m.programmed = None;
    m.pending = None;
}

/// 贴边自动隐藏开关(设置项 island_autohide,缺省=开)
fn autohide_enabled(store: &Store) -> bool {
    store
        .get_setting("island_autohide")
        .map(|v| v != "0")
        .unwrap_or(true)
}

/// 解析 island_pos 设置("x,y" 物理坐标)
fn saved_pos(store: &Store) -> Option<(i32, i32)> {
    store.get_setting("island_pos").and_then(|s| {
        s.split_once(',')
            .and_then(|(a, b)| match (a.trim().parse::<i32>(), b.trim().parse::<i32>()) {
                (Ok(x), Ok(y)) => Some((x, y)),
                _ => None,
            })
    })
}

/// 物理坐标 → 逻辑坐标(按显示器缩放换算)
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

/// 显示器矩形(逻辑坐标):x, y, w, h
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

/// 判定拖放位置应吸附的屏幕边(优先级:上 > 左 > 右;越界超出阈值视为自由位置)。
/// mon = (x, y, w, h) 显示器矩形;独立函数便于几何单测
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

/// 隐藏位坐标:按贴靠边把窗口滑出屏幕,仅留露出常量对应的信息条/半圆标签
/// (显示器以纯数值传入:mon_pos=原点,mon_w=宽度;独立函数便于几何单测)
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

/// 程序化滑动窗口到位(to 为逻辑坐标;steps=1 即瞬时跳变;8~10 步约 130~200ms 动效)。
/// 动画期间产生的 Moved 事件由 IslandMotion.animating 屏蔽,落点由 programmed 消费,
/// 防止"移动 → Moved → 再评估 → 再移动"的自触发循环
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
        m.animating = true;
        m.programmed = Some(to_phys);
    }
    let win = win.clone();
    let motion2 = motion.clone();
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
        // 兜底防卡死:若落点 Moved 事件未如期消费,延时后复位动画标记
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            let mut m = motion2.lock().unwrap();
            if m.programmed == Some(to_phys) {
                m.animating = false;
                m.programmed = None;
            }
        });
    });
}

/// 执行隐藏/显示:滑向隐藏位或停靠位,同步内存状态并向岛窗口广播 island-dock 事件
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
    // 隐藏/显示前强制收回收缩态尺寸:面板展开时贴边隐藏,若按 520 高度渲染,
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

/// 拖放后的贴靠评估:吸附到最近的屏幕边,按设置决定是否滑出隐藏;自由位置则原样记忆。
/// 全程使用逻辑坐标(物理事件坐标按显示器缩放换算),与窗口逻辑尺寸/前端 CSS 一致
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
    // 吸附隐藏前强制收回收缩态尺寸(拖拽时面板可能仍展开,理由同 peek_apply)
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

/// 左键是否按住(拖拽进行中不评估贴靠,防止拖到半路被吸附走)
fn lbutton_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    unsafe { GetAsyncKeyState(VK_LBUTTON.0.into()) as u16 & 0x8000 != 0 }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
            island_drag_start,
            island_metrics
        ])
        .setup(|app| {
            // 自库:安装目录下 %APPDATA%\com.agenttracker.app\agenttracker.db
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let store = Arc::new(Store::open(&dir.join("agenttracker.db"))?);
            app.manage(store.clone());

            let win = app
                .get_webview_window(ISLAND)
                .ok_or_else(|| anyhow::anyhow!("island 窗口未在配置中定义"))?;

            // 毛玻璃:❌不使用 window-vibrancy——Acrylic 是窗口级效果,会把整个
            // 矩形窗口染成磨砂灰,破坏胶囊形态;岛的正确做法=窗口全透明+CSS 自绘
            // 背景(见 App.css)。依赖保留备注:M1 若做全宽岛形态可再启用。

            // 贴边/拖拽运动状态(setup 内创建,commands 与看护线程经 manage 共享);
            // 必须先于 position_island 创建,启动定位要经它防误判
            let motion = Arc::new(Mutex::new(IslandMotion {
                edge: store
                    .get_setting("island_edge")
                    .unwrap_or_else(|| "none".into()),
                ..Default::default()
            }));
            app.manage(motion.clone());

            // 定位:记忆坐标优先,否则顶部居中
            position_island(&win, store.as_ref(), &motion);

            // 窗口移动事件:程序化滑动的落点被消费忽略;用户拖拽则记录待评估位置
            {
                let motion2 = motion.clone();
                win.on_window_event(move |ev| {
                    let tauri::WindowEvent::Moved(pos) = ev else {
                        return;
                    };
                    let mut m = motion2.lock().unwrap();
                    if m.animating {
                        // 动画产生的移动:仅当到达落点时消费并结算动画
                        if m.programmed == Some((pos.x, pos.y)) {
                            m.animating = false;
                            m.programmed = None;
                            drop(m);
                            SLIDE_GEN.fetch_add(1, Ordering::Relaxed);
                        }
                        return;
                    }
                    m.programmed = None;
                    // 用户拖拽:打断在播动画,交给看护线程防抖评估
                    drop(m);
                    SLIDE_GEN.fetch_add(1, Ordering::Relaxed);
                    motion2.lock().unwrap().pending =
                        Some((std::time::Instant::now(), pos.x, pos.y));
                });
            }

            // 贴靠看护线程:位置静默且左键释放(拖放完成)后评估吸附/隐藏
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

            // 启动恢复:上次贴边 + 自动隐藏开启 → 瞬时滑出(仅露边);其余按记忆坐标可见
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
                    }
                }
            }

            // 设置/报表窗口:关闭即隐藏(而非销毁),保证托盘可反复唤起
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

            // 数据清理:按设置周期启动时执行一次(永不=跳过)
            if let Some(days) = store
                .get_setting("cleanup_days")
                .and_then(|v| v.parse::<i64>().ok())
            {
                if days > 0 {
                    let cutoff =
                        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as i64 - days * 86_400_000)
                            .unwrap_or(0);
                    store.cleanup_older_than(cutoff);
                }
            }

            spawn_aggregator(app.handle().clone(), store);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 岛定位:有记忆坐标用之(逻辑坐标,夹取进显示器防丢失);否则按主显示器顶部居中。
/// 必须经 motion 通道移动(slide_to 瞬时档):否则启动定位产生的 Moved 事件
/// 会被看护线程误判为"拖放",把距顶边仅 6px 的岛自动吸附隐藏
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

/// 系统托盘:常驻核心;菜单=显示/隐藏 + 设置 + 退出
fn build_tray(app: &tauri::App) -> anyhow::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;

    let toggle = MenuItem::with_id(app, "toggle", "显示 / 隐藏灵动岛", true, None::<&str>)?;
    let report = MenuItem::with_id(app, "report", "报表…", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &report, &settings, &quit])?;
    TrayIconBuilder::with_id("at-tray")
        .tooltip("AgentTracker")
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

/// 后台聚合线程:10s tick → 快照广播给前端(02-DESIGN §2.3 调度)
fn spawn_aggregator(app: tauri::AppHandle, store: Arc<Store>) {
    std::thread::spawn(move || {
        let mut agg = Aggregator::new(store);
        loop {
            let snap = agg.tick();
            // 前端未监听时 emit 也只是无接收者,不报错
            let _ = app.emit("island-snapshot", &snap);
            std::thread::sleep(Duration::from_secs(10));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 贴边几何判定:阈值内吸附,优先级 上 > 左 > 右
    #[test]
    fn test_detect_edge() {
        let mon = (0, 0, 1920, 1080); // 显示器矩形
        let size = (480, 48);
        // 屏幕中央:自由位置
        assert_eq!(detect_edge((720, 500), mon, size, SNAP_THRESHOLD), "none");
        // 顶部各处(含角落,顶部优先)
        assert_eq!(detect_edge((960, 10), mon, size, SNAP_THRESHOLD), "top");
        assert_eq!(detect_edge((5, 5), mon, size, SNAP_THRESHOLD), "top");
        // 左侧(不在顶部阈值内)
        assert_eq!(detect_edge((3, 500), mon, size, SNAP_THRESHOLD), "left");
        // 右侧:窗口右缘距屏幕右缘 20px <= 24
        assert_eq!(detect_edge((1920 - 460, 500), mon, size, SNAP_THRESHOLD), "right");
        // 恰好等于阈值:仍吸附(<=)
        assert_eq!(detect_edge((24, 500), mon, size, SNAP_THRESHOLD), "left");
        // 超出阈值一个像素:不吸附
        assert_eq!(detect_edge((25, 500), mon, size, SNAP_THRESHOLD), "none");
        // 顶部超出阈值:落空到左侧判断
        assert_eq!(detect_edge((960, 25), mon, size, SNAP_THRESHOLD), "none");
    }

    /// 隐藏位计算:三种贴靠边各露 PEEK_PX 在屏内
    #[test]
    fn test_hidden_pos() {
        let mon_pos = (0, 0);
        let mon_w = 1920;
        let width = island_width(mon_w); // 480
        // 顶部:上滑,底部露出 PEEK_TOP_H(独立信息条)
        assert_eq!(hidden_pos(mon_pos, mon_w, "top", (720, 0), width), (720, -(ISLAND_H - PEEK_TOP_H)));
        // 左侧:左滑,右侧露出 PEEK_SIDE_W(半圆标签)
        assert_eq!(hidden_pos(mon_pos, mon_w, "left", (0, 300), width), (-(width - PEEK_SIDE_W), 300));
        // 右侧:右滑,左缘留在 1920-PEEK_SIDE_W
        assert_eq!(hidden_pos(mon_pos, mon_w, "right", (1440, 300), width), (1920 - PEEK_SIDE_W, 300));
    }

    /// 自适应宽度:按屏幕逻辑宽 30% 夹取 [380, 800]
    #[test]
    fn test_island_width() {
        assert_eq!(island_width(1280), 384);
        assert_eq!(island_width(1366), 410); // 所有者笔记本:+1/5 手感校准
        assert_eq!(island_width(1440), 432);
        assert_eq!(island_width(1920), 576);
        assert_eq!(island_width(2560), 768);
        assert_eq!(island_width(3840), 800); // 大屏封顶
    }
}
