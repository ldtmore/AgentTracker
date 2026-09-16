//! GLM Coding Plan 适配器:查询 5 小时/每周积分窗口用量与重置时间。
//! 接口与响应格式来自本机实测(2026-09-16,见 docs/01-RESEARCH.md §7/§9):
//!   GET {origin}/api/monitor/usage/quota/limit   Header: Authorization: <裸key>
//!   data.limits[] 中 TOKENS_LIMIT+number=5 → 5h 窗口;number=1&unit=6 → 周窗口;
//!   percentage 为已用百分比;nextResetTime 为 Unix 毫秒。

use std::path::PathBuf;

use crate::provider::ProviderAdapter;
use crate::store::QuotaRow;

/// GLM 平台端点(国内/国际)
pub const BASE_BIGMODEL: &str = "https://open.bigmodel.cn";
pub const BASE_ZAI: &str = "https://api.z.ai";

pub struct GlmProvider {
    /// API origin(如 https://open.bigmodel.cn)
    base: String,
    /// Coding Plan API key(与 ANTHROPIC_AUTH_TOKEN 同值)
    token: String,
}

/// 凭据发现结果:来自设置/环境变量/claude-menu 配置
#[derive(Debug)]
pub struct GlmCreds {
    pub base: String,
    pub token: String,
    pub source: &'static str, // 凭据来源,用于设置页展示
}

impl GlmProvider {
    pub fn new(base: &str, token: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            token: token.to_string(),
        }
    }

    /// 凭据发现链(优先级从高到低):
    /// ① 显式传入(应用设置)→ ② 环境变量 → ③ ~\.claude\suppliers.json 自动发现
    /// (claude-menu 的供应商配置文件,选 base 含 bigmodel.cn / z.ai 的条目)
    pub fn discover() -> Option<GlmCreds> {
        // ② 环境变量
        if let (Ok(base), Ok(tok)) = (
            std::env::var("ANTHROPIC_BASE_URL"),
            std::env::var("ANTHROPIC_AUTH_TOKEN"),
        ) {
            if !tok.is_empty() && (base.contains("bigmodel") || base.contains("z.ai")) {
                let origin = base
                    .trim_end_matches('/')
                    .trim_end_matches("/api/anthropic")
                    .to_string();
                return Some(GlmCreds { base: origin, token: tok, source: "环境变量" });
            }
        }
        // ③ suppliers.json(键名带 "env:" 前缀)
        if let Some(c) = discover_from_suppliers() {
            return Some(c);
        }
        None
    }
}

impl ProviderAdapter for GlmProvider {
    fn id(&self) -> &'static str {
        "glm"
    }

    fn fetch_quota(&self) -> anyhow::Result<Vec<QuotaRow>> {
        let url = format!("{}/api/monitor/usage/quota/limit", self.base);
        let resp = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()?
            .get(&url)
            .header("Authorization", &self.token)
            .header("Accept", "application/json")
            .send()?;
        if !resp.status().is_success() {
            anyhow::bail!("GLM 额度接口 HTTP {}", resp.status());
        }
        let body: QuotaResponse = resp.json()?;
        parse_quota(&body)
    }
}

/// 从 claude-menu 的 ~\.claude\suppliers.json 发现 GLM 凭据(只读)
fn discover_from_suppliers() -> Option<GlmCreds> {
    let mut path = PathBuf::from(std::env::var_os("USERPROFILE")?);
    path.push(".claude");
    path.push("suppliers.json");
    let raw = std::fs::read_to_string(path).ok()?;
    let root: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let Some(obj) = root.as_object() else { return None };
    for (_name, entry) in obj {
        let Some(fields) = entry.as_object() else { continue };
        let get = |k: &str| fields.get(&format!("env:{k}")).and_then(|v| v.as_str());
        let (Some(base), Some(tok)) = (get("ANTHROPIC_BASE_URL"), get("ANTHROPIC_AUTH_TOKEN")) else {
            continue;
        };
        if !tok.is_empty() && (base.contains("bigmodel") || base.contains("z.ai")) {
            let origin = base
                .trim_end_matches('/')
                .trim_end_matches("/api/anthropic")
                .to_string();
            return Some(GlmCreds { base: origin, token: tok.to_string(), source: "claude-menu 配置" });
        }
    }
    None
}

// ---------- 响应解析(结构来自 2026-09-16 实测) ----------

#[derive(serde::Deserialize)]
struct QuotaResponse {
    #[serde(default)]
    success: bool,
    data: Option<QuotaData>,
}

#[derive(serde::Deserialize)]
struct QuotaData {
    /// 套餐档位:lite/pro/max
    #[serde(default)]
    #[allow(dead_code)]
    level: Option<String>,
    #[serde(default)]
    limits: Vec<LimitItem>,
}

#[derive(serde::Deserialize)]
struct LimitItem {
    /// TOKENS_LIMIT(积分窗口)| TIME_LIMIT(MCP 工具,M1 处理)
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    number: Option<i64>,
    #[serde(default)]
    unit: Option<i64>,
    #[serde(default)]
    usage: Option<i64>,
    #[serde(default)]
    #[serde(rename = "currentValue")]
    current_value: Option<i64>,
    #[serde(default)]
    remaining: Option<i64>,
    /// 已用百分比(官方口径,实测为"已用"而非"剩余")
    #[serde(default)]
    percentage: Option<f64>,
    #[serde(default)]
    #[serde(rename = "nextResetTime")]
    next_reset_time: Option<i64>,
}

/// 解析响应 → 额度快照(5h + weekly)
fn parse_quota(body: &QuotaResponse) -> anyhow::Result<Vec<QuotaRow>> {
    let data = body
        .data
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("GLM 额度响应缺少 data"))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let mut out = vec![];
    for item in &data.limits {
        if item.kind != "TOKENS_LIMIT" {
            continue; // TIME_LIMIT(MCP)M1 纳入
        }
        // number=5 → 5h 窗口;number=1(周)→ weekly
        let kind = match (item.number, item.unit) {
            (Some(5), _) => "5h",
            (Some(1), Some(6)) => "weekly",
            _ => continue, // 未知窗口类型,忽略以容忍接口演进
        };
        // 已用百分比:官方 percentage 优先,缺失时按 usage/remaining 计算
        let used_percent = item
            .percentage
            .or_else(|| calc_percent(item.usage, item.remaining, item.current_value));
        out.push(QuotaRow {
            provider: "glm".into(),
            window_kind: kind.into(),
            used_percent,
            used_tokens: None, // TOKENS_LIMIT 仅返回百分比,无绝对量
            reset_at: item.next_reset_time,
            fetched_at: now,
        });
    }
    if out.is_empty() {
        anyhow::bail!("GLM 额度响应无可识别的 TOKENS_LIMIT 条目");
    }
    Ok(out)
}

/// percentage 缺失时的兜底:usage/remaining 或 currentValue 口径
fn calc_percent(usage: Option<i64>, remaining: Option<i64>, current: Option<i64>) -> Option<f64> {
    if let (Some(u), Some(r)) = (usage, remaining) {
        let total = u + r;
        if total > 0 {
            return Some((u as f64 / total as f64) * 100.0);
        }
    }
    // currentValue 即已用绝对量,但无 total 时无法换算——返回已用值本身(仅作展示参考)
    current.map(|c| c as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 实测响应样例(2026-09-16 本机,数值已核对:89%=5h,52%=周)
    const REAL_RESP: &str = r#"{
      "code": 200, "msg": "操作成功", "success": true,
      "data": {
        "level": "pro",
        "limits": [
          {"type":"TIME_LIMIT","unit":5,"number":1,"usage":1000,"currentValue":16,"remaining":984,"percentage":1,"nextResetTime":1790407328997},
          {"type":"TOKENS_LIMIT","unit":3,"number":5,"percentage":89,"nextResetTime":1789562676650},
          {"type":"TOKENS_LIMIT","unit":6,"number":1,"percentage":52,"nextResetTime":1789716128992}
        ]
      }
    }"#;

    #[test]
    fn test_parse_real_response() {
        let body: QuotaResponse = serde_json::from_str(REAL_RESP).unwrap();
        let rows = parse_quota(&body).unwrap();
        assert_eq!(rows.len(), 2, "TIME_LIMIT 应被忽略");
        let h5 = rows.iter().find(|r| r.window_kind == "5h").unwrap();
        assert_eq!(h5.used_percent, Some(89.0));
        assert_eq!(h5.reset_at, Some(1789562676650));
        let week = rows.iter().find(|r| r.window_kind == "weekly").unwrap();
        assert_eq!(week.used_percent, Some(52.0));
        assert!(rows.iter().all(|r| r.provider == "glm"));
    }

    #[test]
    fn test_fallback_percent() {
        assert_eq!(calc_percent(Some(16), Some(984), None), Some(1.6));
        assert_eq!(calc_percent(None, None, Some(5)), Some(5.0));
        assert_eq!(calc_percent(None, None, None), None);
    }

    /// 集成:真实调用 Monitor API(手动:cargo test -- --ignored)
    #[test]
    #[ignore]
    fn test_real_glm_fetch() {
        let creds = GlmProvider::discover().expect("应能发现 GLM 凭据(环境变量或 suppliers.json)");
        println!("凭据来源: {}", creds.source);
        let p = GlmProvider::new(&creds.base, &creds.token);
        let rows = p.fetch_quota().unwrap();
        assert_eq!(rows.len(), 2);
        for r in &rows {
            let pct = r.used_percent.expect("TOKENS_LIMIT 应有百分比");
            assert!((0.0..=100.0).contains(&pct), "百分比异常: {pct}");
            assert!(r.reset_at.unwrap_or(0) > 1_700_000_000_000, "重置时间应为毫秒");
            println!("[{}] 已用 {}% | 重置于 {}", r.window_kind, pct,
                r.reset_at.map(|t| chrono::DateTime::from_timestamp_millis(t).map(|d| d.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()).unwrap_or_default());
        }
    }
}
