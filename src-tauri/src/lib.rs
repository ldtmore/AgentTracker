// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
pub mod collector;
pub mod provider;
pub mod state;
pub mod store;

use std::sync::Arc;
use std::time::Duration;

use tauri::{Emitter, Manager};

use crate::state::service::Aggregator;
use crate::store::Store;

/// 岛窗口标签(tauri.conf.json 中定义)
const ISLAND: &str = "island";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // 自库:安装目录下 %APPDATA%\com.agenttracker.app\agenttracker.db
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let store = Arc::new(Store::open(&dir.join("agenttracker.db"))?);

            let win = app
                .get_webview_window(ISLAND)
                .ok_or_else(|| anyhow::anyhow!("island 窗口未在配置中定义"))?;

            // 毛玻璃:❌不使用 window-vibrancy——Acrylic 是窗口级效果,会把整个
            // 矩形窗口染成磨砂灰,破坏胶囊形态;岛的正确做法=窗口全透明+CSS 自绘
            // 背景(见 App.css)。依赖保留备注:M1 若做全宽岛形态可再启用。

            // 定位:记忆坐标优先,否则顶部居中
            position_island(&win, store.as_ref());

            // 拖拽位置记忆:窗口移动即持久化坐标(节流:直接写,SQLite WAL 足够轻)
            {
                let store2 = store.clone();
                win.on_window_event(move |ev| {
                    if let tauri::WindowEvent::Moved(pos) = ev {
                        let _ = store2.set_setting("island_pos", &format!("{},{}", pos.x, pos.y));
                    }
                });
            }

            build_tray(app)?;
            spawn_aggregator(app.handle().clone(), store);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 岛定位:有记忆坐标用之;否则按主显示器顶部居中(y 偏移 6px)
fn position_island(win: &tauri::WebviewWindow, store: &Store) {
    if let Some(pos) = store.get_setting("island_pos") {
        if let Some((x, y)) = pos.split_once(',') {
            if let (Ok(x), Ok(y)) = (x.trim().parse::<i32>(), y.trim().parse::<i32>()) {
                let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
                return;
            }
        }
    }
    let Ok(Some(monitor)) = win.current_monitor() else { return };
    let size = monitor.size();
    let mpos = monitor.position();
    let wlen = win.outer_size().map(|s| s.width as i32).unwrap_or(480);
    let x = mpos.x + (size.width as i32 - wlen) / 2;
    let y = mpos.y + 6;
    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
}

/// 系统托盘:常驻核心;M0 菜单=显示/隐藏 + 退出
fn build_tray(app: &tauri::App) -> anyhow::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;

    let toggle = MenuItem::with_id(app, "toggle", "显示 / 隐藏灵动岛", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &quit])?;
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
