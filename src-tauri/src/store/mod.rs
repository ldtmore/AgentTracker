//! 本地存储层:AgentTracker 自库(SQLite)的打开/迁移/读写封装。
//! 设计依据 docs/02-DESIGN.md §3;红线③(顺序无关)由幂等键与水位保证。
//! 线程模型:Connection 非 Sync,用 Mutex 包裹,单写多读经同一锁串行(M0 规模足够)。

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

/// 初始化迁移脚本(0001)
const MIGRATION_0001: &str = include_str!("migrations/0001_init.sql");

/// 一条 token 用量流水(来自任一 Agent 适配器的增量采集)
#[derive(Debug, Clone)]
pub struct UsageRow {
    pub session_id: String,
    pub agent: String,
    pub model: String,
    pub provider: Option<String>,
    pub ts: i64, // Unix 毫秒
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_creation_tokens: Option<i64>,
    pub duration_ms: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub error_type: Option<String>,
}

/// 一条额度快照(来自 Provider 适配器)
#[derive(Debug, Clone)]
pub struct QuotaRow {
    pub provider: String,
    pub window_kind: String, // '5h' | 'weekly'
    pub used_percent: Option<f64>,
    pub used_tokens: Option<i64>,
    pub reset_at: Option<i64>,
    pub fetched_at: i64,
}

/// 存储句柄:克隆 Arc 后全局共享
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// 打开(或创建)数据库并执行迁移;父目录不存在时自动创建
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::migrate(&conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// 迁移:按 user_version 顺序执行(M0 仅 0001,后续版本递增)
    fn migrate(conn: &Connection) -> rusqlite::Result<()> {
        let ver: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if ver < 1 {
            conn.execute_batch(MIGRATION_0001)?;
            conn.pragma_update(None, "user_version", 1)?;
        }
        Ok(())
    }

    /// 读取某 Agent 的采集水位(时间戳,毫秒);无记录返回 0
    pub fn get_watermark(&self, agent: &str) -> i64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT last_ts FROM watermarks WHERE agent = ?1",
            params![agent],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten()
        .unwrap_or(0)
    }

    /// 更新采集水位(仅前进,不回退)
    pub fn set_watermark(&self, agent: &str, ts: i64) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO watermarks(agent, last_ts) VALUES(?1, ?2)
             ON CONFLICT(agent) DO UPDATE SET last_ts = MAX(last_ts, excluded.last_ts)",
            params![agent, ts],
        );
    }

    /// upsert 会话元数据(首见时间不覆盖,最新状态全量刷新)
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_session(
        &self,
        id: &str,
        agent: &str,
        provider: Option<&str>,
        model: Option<&str>,
        project_dir: Option<&str>,
        title: Option<&str>,
        last_seen_at: i64,
        state: &str,
        state_reason: Option<&str>,
    ) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO sessions(id, agent, provider, model, project_dir, title,
                                  first_seen_at, last_seen_at, state, state_reason)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?7,?8,?9)
             ON CONFLICT(id) DO UPDATE SET
               provider = COALESCE(excluded.provider, provider),
               model    = COALESCE(excluded.model, model),
               project_dir = COALESCE(excluded.project_dir, project_dir),
               title    = COALESCE(excluded.title, title),
               last_seen_at = MAX(last_seen_at, excluded.last_seen_at),
               state = excluded.state,
               state_reason = excluded.state_reason",
            params![id, agent, provider, model, project_dir, title, last_seen_at, state, state_reason],
        );
    }

    /// 幂等插入用量流水:同幂等键(agent+session+ts+model)冲突时,仅当新行四项
    /// 用量合计更大才整行覆盖——与 Claude Code"同消息保留最大快照"口径一致,
    /// 跨 tick 重采到更完整的流式快照时能原地升级而非被 INSERT OR IGNORE 顶掉;
    /// 返回实际变更行数(新插入或覆盖)
    pub fn insert_usage(&self, rows: &[UsageRow]) -> usize {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().unwrap();
        let mut changed = 0usize;
        for r in rows {
            let n = tx
                .execute(
                    "INSERT INTO usage_records(
                       session_id, agent, model, provider, ts,
                       input_tokens, output_tokens, reasoning_tokens,
                       cache_read_tokens, cache_creation_tokens,
                       duration_ms, ttft_ms, error_type)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
                     ON CONFLICT(agent, session_id, ts, model) DO UPDATE SET
                       input_tokens = excluded.input_tokens,
                       output_tokens = excluded.output_tokens,
                       reasoning_tokens = excluded.reasoning_tokens,
                       cache_read_tokens = excluded.cache_read_tokens,
                       cache_creation_tokens = excluded.cache_creation_tokens,
                       duration_ms = excluded.duration_ms,
                       ttft_ms = excluded.ttft_ms,
                       error_type = excluded.error_type
                     WHERE (COALESCE(excluded.input_tokens,0) + COALESCE(excluded.output_tokens,0)
                          + COALESCE(excluded.cache_read_tokens,0) + COALESCE(excluded.cache_creation_tokens,0))
                           >
                           (COALESCE(input_tokens,0) + COALESCE(output_tokens,0)
                          + COALESCE(cache_read_tokens,0) + COALESCE(cache_creation_tokens,0))",
                    params![
                        r.session_id, r.agent, r.model, r.provider, r.ts,
                        r.input_tokens, r.output_tokens, r.reasoning_tokens,
                        r.cache_read_tokens, r.cache_creation_tokens,
                        r.duration_ms, r.ttft_ms, r.error_type
                    ],
                )
                .unwrap_or(0);
            changed += n;
        }
        tx.commit().unwrap();
        changed
    }

    /// 插入额度快照
    pub fn insert_quota(&self, row: &QuotaRow) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO quota_snapshots(provider, window_kind, used_percent, used_tokens, reset_at, fetched_at)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![row.provider, row.window_kind, row.used_percent, row.used_tokens, row.reset_at, row.fetched_at],
        );
    }

    /// 插入原始状态事件(hooks/采集审计)
    pub fn insert_status_event(&self, agent: &str, session_id: Option<&str>, hook: &str, payload: &str, ts: i64) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO status_events(agent, session_id, hook, payload, ts) VALUES(?1,?2,?3,?4,?5)",
            params![agent, session_id, hook, payload, ts],
        );
    }

    /// 设置项读写
    pub fn get_setting(&self, key: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT value FROM app_settings WHERE key = ?1", params![key], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn set_setting(&self, key: &str, value: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO app_settings(key, value) VALUES(?1,?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        );
    }

    /// 某会话的累计 token(input+output,展示口径)
    pub fn session_usage_total(&self, session_id: &str) -> i64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COALESCE(SUM(COALESCE(input_tokens,0)+COALESCE(output_tokens,0)),0)
             FROM usage_records WHERE session_id = ?1",
            params![session_id],
            |r| r.get(0),
        )
        .unwrap_or(0)
    }

    /// 每个供应商+窗口的最新额度快照
    pub fn latest_quotas(&self) -> Vec<QuotaRow> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT provider, window_kind, used_percent, used_tokens, reset_at, fetched_at
             FROM quota_snapshots q
             WHERE id = (SELECT MAX(id) FROM quota_snapshots
                          WHERE provider=q.provider AND window_kind=q.window_kind)",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let rows = stmt.query_map([], |r| {
            Ok(QuotaRow {
                provider: r.get(0)?,
                window_kind: r.get(1)?,
                used_percent: r.get(2)?,
                used_tokens: r.get(3)?,
                reset_at: r.get(4)?,
                fetched_at: r.get(5)?,
            })
        });
        match rows {
            Ok(it) => it.filter_map(|x| x.ok()).collect(),
            Err(_) => vec![],
        }
    }

    /// 某会话最近一次调用所用模型(Claude Code 的 scan 阶段拿不到 model,展示时兜底回填)
    pub fn latest_session_model(&self, session_id: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT model FROM usage_records WHERE session_id = ?1 ORDER BY ts DESC LIMIT 1",
            params![session_id],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten()
    }

    /// 查会话元数据(project_dir/agent),跳转窗口用
    pub fn get_session_meta(&self, id: &str) -> Option<(String, Option<String>)> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT agent, project_dir FROM sessions WHERE id = ?1",
            params![id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
        )
        .optional()
        .ok()
        .flatten()
    }

    /// 读取全部设置(设置页展示)
    pub fn all_settings(&self) -> std::collections::HashMap<String, String> {
        let conn = self.conn.lock().unwrap();
        let Ok(mut stmt) = conn.prepare("SELECT key, value FROM app_settings") else {
            return std::collections::HashMap::new();
        };
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)));
        match rows {
            Ok(it) => it.filter_map(|x| x.ok()).collect(),
            Err(_) => std::collections::HashMap::new(),
        }
    }

    /// 数据清理:删除 before_ts 之前的用量/快照/事件(设置页滚动周期用)
    pub fn cleanup_older_than(&self, before_ts: i64) -> u64 {
        let conn = self.conn.lock().unwrap();
        let mut total = 0usize;
        for sql in [
            "DELETE FROM usage_records WHERE ts < ?1",
            "DELETE FROM quota_snapshots WHERE fetched_at < ?1",
            "DELETE FROM status_events WHERE ts < ?1",
        ] {
            total += conn.execute(sql, params![before_ts]).unwrap_or(0);
        }
        total as u64
    }
}

// ===== 报表聚合查询(M1-1) =====

/// 按日聚合用量(日界取本机时区,由 SQLite 'localtime' 修饰符读 OS 时区)
#[derive(Debug, Clone, serde::Serialize)]
pub struct DayUsage {
    pub day: String, // "2026-09-17"
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
}

/// 按单一维度(模型/供应商)聚合的 token 总量(四项全口径)
#[derive(Debug, Clone, serde::Serialize)]
pub struct SliceUsage {
    pub label: String,
    pub total: i64,
}

/// 热力图单元:星期×小时的 token 总量
#[derive(Debug, Clone, serde::Serialize)]
pub struct HeatCell {
    pub weekday: i32, // 0=周日 … 6=周六(SQLite strftime %w)
    pub hour: i32,    // 0–23
    pub total: i64,
}

/// 报表时间范围起点(days<=0 表示全部历史;否则 days 天前的毫秒时间戳)
fn range_cutoff(days: i64) -> i64 {
    if days <= 0 {
        return 0;
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64 - days * 86_400_000)
        .unwrap_or(0)
}

impl Store {
    /// 报表:按日聚合,四项用量分列(趋势图堆叠用)
    pub fn report_daily(&self, days: i64) -> Vec<DayUsage> {
        let conn = self.conn.lock().unwrap();
        let Ok(mut stmt) = conn.prepare(
            "SELECT date(ts/1000,'unixepoch','localtime') AS d,
                    SUM(COALESCE(input_tokens,0)), SUM(COALESCE(output_tokens,0)),
                    SUM(COALESCE(cache_read_tokens,0)), SUM(COALESCE(cache_creation_tokens,0))
             FROM usage_records WHERE ts >= ?1 GROUP BY d ORDER BY d",
        ) else {
            return vec![];
        };
        let rows = stmt.query_map([range_cutoff(days)], |r| {
            Ok(DayUsage {
                day: r.get(0)?,
                input: r.get(1)?,
                output: r.get(2)?,
                cache_read: r.get(3)?,
                cache_creation: r.get(4)?,
            })
        });
        match rows {
            Ok(it) => it.filter_map(|x| x.ok()).collect(),
            Err(_) => vec![],
        }
    }

    /// 报表:按模型聚合占比
    pub fn report_by_model(&self, days: i64) -> Vec<SliceUsage> {
        self.report_slice(days, "model")
    }

    /// 报表:按供应商聚合占比(provider 为 NULL 计入 'unknown')
    pub fn report_by_provider(&self, days: i64) -> Vec<SliceUsage> {
        self.report_slice(days, "provider")
    }

    /// 报表:按维度聚合内部实现(label_expr 仅允许内部传入列名,不接外部输入)。
    /// 模型名按小写归一(本机历史数据存在 GLM-5.3/glm-5.3 大小写混用,避免切成两块)
    fn report_slice(&self, days: i64, label_expr: &str) -> Vec<SliceUsage> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT COALESCE(LOWER({label_expr}),'unknown') AS label,
                    SUM(COALESCE(input_tokens,0)+COALESCE(output_tokens,0)
                       +COALESCE(cache_read_tokens,0)+COALESCE(cache_creation_tokens,0)) AS total
             FROM usage_records WHERE ts >= ?1 GROUP BY label ORDER BY total DESC"
        );
        let Ok(mut stmt) = conn.prepare(&sql) else {
            return vec![];
        };
        let rows = stmt.query_map([range_cutoff(days)], |r| {
            Ok(SliceUsage {
                label: r.get(0)?,
                total: r.get(1)?,
            })
        });
        match rows {
            Ok(it) => it.filter_map(|x| x.ok()).collect(),
            Err(_) => vec![],
        }
    }

    /// 报表:星期×小时用量热力图(本机时区)
    pub fn report_heatmap(&self, days: i64) -> Vec<HeatCell> {
        let conn = self.conn.lock().unwrap();
        let Ok(mut stmt) = conn.prepare(
            "SELECT CAST(strftime('%w', ts/1000,'unixepoch','localtime') AS INTEGER),
                    CAST(strftime('%H', ts/1000,'unixepoch','localtime') AS INTEGER),
                    SUM(COALESCE(input_tokens,0)+COALESCE(output_tokens,0)
                       +COALESCE(cache_read_tokens,0)+COALESCE(cache_creation_tokens,0))
             FROM usage_records WHERE ts >= ?1 GROUP BY 1,2",
        ) else {
            return vec![];
        };
        let rows = stmt.query_map([range_cutoff(days)], |r| {
            Ok(HeatCell {
                weekday: r.get(0)?,
                hour: r.get(1)?,
                total: r.get(2)?,
            })
        });
        match rows {
            Ok(it) => it.filter_map(|x| x.ok()).collect(),
            Err(_) => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 生成临时测试库路径(进程级唯一,避免并行测试互踩)
    fn tmp_db(tag: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("at-test-{}-{}.db", tag, std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    fn sample_usage(ts: i64) -> UsageRow {
        UsageRow {
            session_id: "zcode:abc".into(),
            agent: "zcode".into(),
            model: "glm-5.3".into(),
            provider: Some("glm".into()),
            ts,
            input_tokens: Some(1000),
            output_tokens: Some(200),
            reasoning_tokens: Some(50),
            cache_read_tokens: Some(3000),
            cache_creation_tokens: Some(0),
            duration_ms: Some(7812),
            ttft_ms: Some(900),
            error_type: None,
        }
    }

    #[test]
    fn test_open_and_migrate() {
        let path = tmp_db("migrate");
        {
            let store = Store::open(&path).unwrap();
            store.set_setting("k", "v");
            assert_eq!(store.get_setting("k").as_deref(), Some("v"));
        }
        // 重复打开:迁移幂等,数据仍在
        let store2 = Store::open(&path).unwrap();
        assert_eq!(store2.get_setting("k").as_deref(), Some("v"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_usage_idempotent() {
        let path = tmp_db("idem");
        let store = Store::open(&path).unwrap();
        let rows = vec![sample_usage(1_000), sample_usage(2_000)];
        assert_eq!(store.insert_usage(&rows), 2);
        // 同一批再插:全部命中幂等键,0 行新增
        assert_eq!(store.insert_usage(&rows), 0);
        // 交错重复:仅新行入库
        let mut again = rows.clone();
        again.push(sample_usage(3_000));
        assert_eq!(store.insert_usage(&again), 1);
        let _ = std::fs::remove_file(&path);
    }

    /// 同幂等键多快照:仅四项合计更大的行才覆盖(与 CC 流式去重口径一致);
    /// 更小快照重复采集不回退
    #[test]
    fn test_usage_upsert_keeps_max_snapshot() {
        let path = tmp_db("upsert");
        let store = Store::open(&path).unwrap();
        // 首插:流式中途的小快照
        assert_eq!(store.insert_usage(&[sample_usage(1_000)]), 1);
        // 同键更大快照(流式写全):覆盖
        let mut bigger = sample_usage(1_000);
        bigger.input_tokens = Some(5_000);
        assert_eq!(store.insert_usage(&[bigger]), 1);
        // 库中为覆盖后的值(session_usage_total = input+output)
        assert_eq!(store.session_usage_total("zcode:abc"), 5_200);
        // 更小快照重复采到:不覆盖、不变更
        assert_eq!(store.insert_usage(&[sample_usage(1_000)]), 0);
        assert_eq!(store.session_usage_total("zcode:abc"), 5_200);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_watermark_monotonic() {
        let path = tmp_db("wm");
        let store = Store::open(&path).unwrap();
        assert_eq!(store.get_watermark("zcode"), 0);
        store.set_watermark("zcode", 500);
        store.set_watermark("zcode", 300); // 回退值不生效
        assert_eq!(store.get_watermark("zcode"), 500);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_cleanup() {
        let path = tmp_db("clean");
        let store = Store::open(&path).unwrap();
        store.insert_usage(&[sample_usage(1_000), sample_usage(9_000)]);
        store.insert_quota(&QuotaRow {
            provider: "glm".into(),
            window_kind: "5h".into(),
            used_percent: Some(35.0),
            used_tokens: None,
            reset_at: None,
            fetched_at: 800,
        });
        let removed = store.cleanup_older_than(5_000);
        assert_eq!(removed, 2); // 1 条 usage + 1 条快照
        let _ = std::fs::remove_file(&path);
    }

    /// 报表聚合:模型/供应商/日/热力图;总量守恒与排序(TZ 无关断言为主)
    #[test]
    fn test_report_aggregates() {
        let path = tmp_db("report");
        let store = Store::open(&path).unwrap();
        let row_sum = 4_200i64; // sample_usage 四项之和(1000+200+3000+0)
        let r1 = sample_usage(1_000);
        let mut r2 = sample_usage(2_000);
        r2.session_id = "zcode:b".into();
        r2.model = "glm-5.3-flash".into();
        let r3 = sample_usage(200_000); // 与 r1 同模型不同会话/时间
        store.insert_usage(&[r1.clone(), r2.clone(), r3]);

        // 按模型:glm-5.3 两行合计在前(降序),flash 在后
        let models = store.report_by_model(0);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].label, "glm-5.3");
        assert_eq!(models[0].total, row_sum * 2);
        assert_eq!(models[1].label, "glm-5.3-flash");
        assert_eq!(models[1].total, row_sum);

        // 按供应商:全部 glm → 单条
        let providers = store.report_by_provider(0);
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].label, "glm");
        assert_eq!(providers[0].total, row_sum * 3);

        // 热力图:总量守恒,cell 落在合法范围
        let heat = store.report_heatmap(0);
        assert_eq!(heat.iter().map(|c| c.total).sum::<i64>(), row_sum * 3);
        assert!(heat.iter().all(|c| (0..=6).contains(&c.weekday) && (0..=23).contains(&c.hour)));

        // 按日:日字符串格式、总量守恒
        let daily = store.report_daily(0);
        assert!(!daily.is_empty());
        let sum: i64 = daily
            .iter()
            .map(|d| d.input + d.output + d.cache_read + d.cache_creation)
            .sum();
        assert_eq!(sum, row_sum * 3);
        assert!(daily.windows(2).all(|w| w[0].day < w[1].day), "按日升序");
        let _ = std::fs::remove_file(&path);
    }
}
