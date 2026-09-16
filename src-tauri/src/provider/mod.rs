//! Provider(供应商)适配器模块:额度查询抽象。
//! 每个供应商(GLM、Anthropic 官方……)实现一份;唯一允许的对外网络请求(宪法§二)。

pub mod glm;

use crate::store::QuotaRow;

/// 供应商适配器:拉取订阅额度快照
pub trait ProviderAdapter: Send + Sync {
    /// 供应商标识:'glm' | 'anthropic' | ...
    fn id(&self) -> &'static str;

    /// 拉取额度快照(可能多条:5h 窗口 + 周窗口);
    /// 实现必须自带超时,失败返回 Err 由调用方降级(显示最近快照)
    fn fetch_quota(&self) -> anyhow::Result<Vec<QuotaRow>>;
}
