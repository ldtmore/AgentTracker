//! 极简文件日志（2026-09-17 审查优化 1.1）：
//! `log` 门面（已是 tauri 传递依赖，零新增外部依赖）+ std 纯实现，
//! 写入 `<app_data>\logs\agenttrackerisland.log`，超 1MB 滚动为 `.old`。
//! 动机：采集/额度/写库失败此前被静默吞掉，"静默降级"变成了"静默失明"，
//! 岛冻结时无从排查——所有降级路径从此必须留痕。
//!
//! 用法：setup 里 `logging：：init(&app_data_dir)` 一次；其余代码直接 `log：：warn!/info!`。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// 单文件上限，超过滚动为 .old（保留一代，足够排查近期问题）
const MAX_LOG_BYTES: u64 = 1024 * 1024;

struct FileLogger {
    /// 日志文件路径（Mutex 仅包路径，逐条写时开关文件，量级为每 10s 数条，可接受）
    path: PathBuf,
}

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        log::max_level() >= metadata.level()
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        // 时间戳用本机时区（chrono 已是依赖）；格式化失败不致命，跳过该条
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let line = format!(
            "[{ts}] [{}] [{}] {}\n",
            record.level(),
            record.target(),
            record.args()
        );
        // 滚动：超限则 rename 为 .old（先删旧档；rename 竞争失败静默，下条日志再试）
        if let Ok(meta) = fs::metadata(&self.path) {
            if meta.len() > MAX_LOG_BYTES {
                let old = self.path.with_extension("log.old");
                let _ = fs::remove_file(&old);
                let _ = fs::rename(&self.path, &old);
            }
        }
        // 日志实现内绝不允许 panic（append 失败静默丢弃该条）
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = f.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {}
}

/// 初始化全局日志。级别：环境变量 `AT_LOG=debug` 时 debug，默认 info。
/// 重复调用无害（仅首次生效）。
pub fn init(log_dir: &Path) {
    let _ = fs::create_dir_all(log_dir);
    let path = log_dir.join("agenttrackerisland.log");
    let level = match std::env::var("AT_LOG").as_deref() {
        Ok("debug") | Ok("trace") => log::LevelFilter::Debug,
        _ => log::LevelFilter::Info,
    };
    // set_boxed_logger 仅可成功一次；失败说明已初始化，忽略
    if log::set_boxed_logger(Box::new(FileLogger { path })).is_ok() {
        log::set_max_level(level);
        log::info!("日志系统初始化完成（级别 {level}）");
    }
}

/// 安装全局 panic 钩子：任何线程 panic 先落盘再走原钩子，
/// 与聚合线程的 catch_unwind 配合（审查 1.1：panic 必须留痕）。
pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let loc = info
            .location()
            .map(|l| l.to_string())
            .unwrap_or_else(|| "未知位置".into());
        let msg = payload_str(info.payload());
        log::error!("panic @ {loc}: {msg}");
        prev(info);
    }));
}

/// 提取 panic payload 的可读文本（&str/String/其他）
fn payload_str(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "非字符串 panic".into()
    }
}

/// 供 spawn_aggregator 打印 catch_unwind 捕获的 payload
pub fn panic_payload_str(p: Box<dyn std::any::Any + Send>) -> String {
    payload_str(p.as_ref())
}
