/**
 * 灵动岛背景不透明度：设置页【灵动岛】节单滑块调基准值（存储键 island_opacity，
 * 0–100 整数字符串），三层背景按固定偏移派生 alpha，视觉层级恒定——
 * 面板永远比胶囊实一档（文字密集，可读性要求更高），
 * 隐藏态比胶囊淡一档（隐藏 = 低调弱化，可见性由不透明的 Agent 身份色块承担）。
 * 应用方式与主题同构：派生 alpha 写根元素 CSS 变量，App.css 以
 * rgba(var(--pill-rgb), var(--pill-alpha)) 形式消费；深浅主题只换 RGB，alpha 通用。
 */

/** 设置存储键（app_settings KV 表，纯前端约定，Rust 端通用透传） */
export const ISLAND_OPACITY_KEY = "island_opacity";

/** 设置页拖动后广播给岛窗口的事件名（emit 全窗口，岛窗口监听即时生效） */
export const ISLAND_OPACITY_EVENT = "island-opacity-changed";

/** 默认基准不透明度（%）：与历史深色胶囊 alpha 0.72 一致，老用户无感迁移 */
export const ISLAND_OPACITY_DEFAULT = 72;

/** 滑块极限：下限 30% 保证深色底叠浅色桌面时文字对比度 ≥ 约 4:1，杜绝完全透明不可见 */
export const ISLAND_OPACITY_MIN = 30;
export const ISLAND_OPACITY_MAX = 100;

/** 派生偏移（alpha 绝对值）：面板比胶囊实一档 */
const PANEL_OFFSET = 0.16;
/** 派生偏移：隐藏态比胶囊淡一档（2026-09-17 定版：反转历史"隐藏态最实"的旧关系） */
const EDGE_OFFSET = -0.1;
/** 隐藏态独立下限：缝隙描边与额度发丝线在花哨壁纸上仍可辨 */
const EDGE_MIN = 0.45;

/** 收窄任意值为合法基准：非数字/越界一律钳到区间内（脏数据防御，参照 sanitizeThresholds） */
export function asIslandOpacity(v: unknown): number {
  const n = Number(v);
  if (!Number.isFinite(n)) return ISLAND_OPACITY_DEFAULT;
  return Math.min(ISLAND_OPACITY_MAX, Math.max(ISLAND_OPACITY_MIN, Math.round(n)));
}

/** 基准 % → 三层派生 alpha（各层 clamp 到 0–1；隐藏态另有独立下限） */
export function deriveAlphas(base: number): { pill: number; panel: number; edge: number } {
  const a = base / 100;
  const clamp01 = (x: number) => Math.min(1, Math.max(0, x));
  return {
    pill: clamp01(a),
    panel: clamp01(a + PANEL_OFFSET),
    edge: Math.max(EDGE_MIN, clamp01(a + EDGE_OFFSET)),
  };
}

/** 把派生 alpha 写到根元素 CSS 变量（inline style 优先级最高，主题切换只换 RGB 互不干扰） */
export function applyIslandOpacity(base: number): void {
  const { pill, panel, edge } = deriveAlphas(base);
  const root = document.documentElement.style;
  root.setProperty("--pill-alpha", pill.toFixed(2));
  root.setProperty("--panel-alpha", panel.toFixed(2));
  root.setProperty("--edge-alpha", edge.toFixed(2));
}
