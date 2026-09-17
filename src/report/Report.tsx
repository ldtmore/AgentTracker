/**
 * 报表页（M1-1）：token 用量统计窗口 —— 每日趋势 / 周×小时热力图 / 模型与供应商占比
 * 数据来自本地 SQLite(usage_records)，Rust 侧聚合，前端 ECharts 按需渲染；
 * 时间口径为本机时区（Asia/Shanghai），范围切换重新拉取
 */
import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import * as echarts from "echarts/core";
import type { EChartsCoreOption } from "echarts/core";
import { BarChart, HeatmapChart, PieChart } from "echarts/charts";
import {
  GridComponent,
  LegendComponent,
  TooltipComponent,
  VisualMapComponent,
} from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";
import Chart from "./Chart";
import { fmtTokens } from "../shared/types";
import { useTheme } from "../shared/theme";
import "./report.css";

// 按需注册用到的图表与组件（01-RESEARCH §9，减小 bundle）
echarts.use([
  BarChart,
  HeatmapChart,
  PieChart,
  GridComponent,
  LegendComponent,
  TooltipComponent,
  VisualMapComponent,
  CanvasRenderer,
]);

/** 每日聚合行（对应 Rust store：：DayUsage） */
interface DayUsage {
  day: string;
  input: number;
  output: number;
  cache_read: number;
  cache_creation: number;
}

/** 模型/供应商占比行（对应 store：：SliceUsage） */
interface SliceUsage {
  label: string;
  total: number;
}

/** 热力图单元（对应 store：：HeatCell） */
interface HeatCell {
  weekday: number; // 0=周日
  hour: number;
  total: number;
}

/** 时间范围选项（days=0 表示全部历史） */
const RANGES = [
  { label: "近 7 天", days: 7 },
  { label: "近 30 天", days: 30 },
  { label: "近 90 天", days: 90 },
  { label: "全部", days: 0 },
];

const WEEKDAYS = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];

/** 堆叠分类与配色（暗色科技风，与岛面板一致） */
const STACK = [
  { key: "input" as const, name: "输入", color: "#60a5fa" },
  { key: "output" as const, name: "输出", color: "#34d399" },
  { key: "cache_read" as const, name: "缓存读", color: "#a78bfa" },
  { key: "cache_creation" as const, name: "缓存写", color: "#fbbf24" },
];

const PIE_COLORS = ["#34d399", "#60a5fa", "#a78bfa", "#fbbf24", "#f87171", "#38bdf8", "#f472b6"];

export default function Report() {
  const theme = useTheme();
  const [days, setDays] = useState(30);
  const [daily, setDaily] = useState<DayUsage[]>([]);
  const [models, setModels] = useState<SliceUsage[]>([]);
  const [providers, setProviders] = useState<SliceUsage[]>([]);
  const [heat, setHeat] = useState<HeatCell[]>([]);
  const [loaded, setLoaded] = useState(false);

  // 范围切换即全量重拉（本地查询，毫秒级）
  useEffect(() => {
    let alive = true;
    setLoaded(false);
    (async () => {
      try {
        const [d, m, p, h] = await Promise.all([
          invoke<DayUsage[]>("report_daily", { days }),
          invoke<SliceUsage[]>("report_by_model", { days }),
          invoke<SliceUsage[]>("report_by_provider", { days }),
          invoke<HeatCell[]>("report_heatmap", { days }),
        ]);
        if (!alive) return;
        setDaily(d);
        setModels(m);
        setProviders(p);
        setHeat(h);
      } finally {
        if (alive) setLoaded(true);
      }
    })();
    return () => {
      alive = false;
    };
  }, [days]);

  // 图表主题色（M1-4）：轴线/图例文字、网格线、饼图标签随主题切换；
  // 序列配色（STACK/PIE_COLORS/热力色阶）为高饱和色，双主题通用不再拆分
  const axisText = { color: theme === "dark" ? "#9ca3af" : "#57606a" };
  const splitLine = theme === "dark" ? "rgba(255,255,255,0.06)" : "rgba(0,0,0,0.08)";
  const pieLabel = theme === "dark" ? "#d1d5db" : "#424a53";

  // 趋势图：四项用量堆叠柱
  const trendOption = useMemo<EChartsCoreOption>(
    () => ({
      tooltip: { trigger: "axis" },
      legend: { data: STACK.map((s) => s.name), textStyle: axisText, top: 0 },
      grid: { left: 56, right: 16, top: 32, bottom: 24 },
      xAxis: {
        type: "category",
        data: daily.map((d) => d.day.slice(5)),
        axisLabel: axisText,
      },
      yAxis: {
        type: "value",
        axisLabel: { ...axisText, formatter: (v: number) => fmtTokens(v) },
        splitLine: { lineStyle: { color: splitLine } },
      },
      series: STACK.map((s) => ({
        name: s.name,
        type: "bar",
        stack: "total",
        data: daily.map((d) => d[s.key]),
        itemStyle: { color: s.color },
        barMaxWidth: 26,
      })),
    }),
    [daily, theme],
  );

  // 热力图：列=小时，行=星期，色阶=token 总量
  const heatOption = useMemo<EChartsCoreOption>(() => {
    const max = heat.reduce((m, c) => Math.max(m, c.total), 0);
    return {
      tooltip: {
        formatter: (p: { data: [number, number, number] }) =>
          `${WEEKDAYS[p.data[1]]} ${p.data[0]}:00 · ${fmtTokens(p.data[2])}`,
      },
      grid: { left: 44, right: 20, top: 10, bottom: 52 },
      xAxis: {
        type: "category",
        data: Array.from({ length: 24 }, (_, i) => `${i}`),
        axisLabel: axisText,
        splitArea: { show: true },
      },
      yAxis: { type: "category", data: WEEKDAYS, axisLabel: axisText },
      visualMap: {
        min: 0,
        max: Math.max(max, 1),
        calculable: true,
        orient: "horizontal",
        left: "center",
        bottom: 0,
        textStyle: axisText,
        inRange: { color: ["#1e3a5f", "#38bdf8", "#34d399"] },
      },
      series: [
        {
          type: "heatmap",
          data: heat.map((c) => [c.hour, c.weekday, c.total]),
        },
      ],
    };
  }, [heat, theme]);

  // 占比饼图（模型/供应商共用模板）
  const pieOption = (data: SliceUsage[]): EChartsCoreOption => ({
    tooltip: {
      formatter: (p: { name: string; value: number; percent: number }) =>
        `${p.name} · ${fmtTokens(p.value)}(${p.percent}%)`,
    },
    legend: { bottom: 0, textStyle: axisText, type: "scroll" },
    color: PIE_COLORS,
    series: [
      {
        type: "pie",
        radius: ["38%", "66%"],
        center: ["50%", "44%"],
        data: data.map((d) => ({ name: d.label, value: d.total })),
        label: { color: pieLabel, formatter: "{d}%" },
      },
    ],
  });

  const empty = loaded && daily.length === 0 && models.length === 0;

  return (
    <div className="rp-root">
      <div className="rp-header">
        <span className="rp-title">用量报表</span>
        <div className="rp-ranges">
          {RANGES.map((r) => (
            <button
              key={r.days}
              className={`rp-btn${days === r.days ? " active" : ""}`}
              onClick={() => setDays(r.days)}
            >
              {r.label}
            </button>
          ))}
        </div>
      </div>

      {empty ? (
        <div className="rp-empty">所选范围内暂无数据</div>
      ) : (
        <>
          <section className="rp-card">
            <div className="rp-card-title">每日 Token 趋势（堆叠：输入/输出/缓存）</div>
            <Chart option={trendOption} height={280} />
          </section>
          <section className="rp-card">
            <div className="rp-card-title">周 × 小时用量热力图（本机时区）</div>
            <Chart option={heatOption} height={250} />
          </section>
          <div className="rp-grid">
            <section className="rp-card">
              <div className="rp-card-title">按模型占比</div>
              <Chart option={pieOption(models)} height={260} />
            </section>
            <section className="rp-card">
              <div className="rp-card-title">按供应商占比</div>
              <Chart option={pieOption(providers)} height={260} />
            </section>
          </div>
          <div className="rp-foot">
            统计口径:输入 + 输出 + 缓存读写的全量 token · 数据源为本地 SQLite,不代表官方计费
          </div>
        </>
      )}
    </div>
  );
}
