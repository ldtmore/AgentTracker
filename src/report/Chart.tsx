/**
 * ECharts React 薄封装（01-RESEARCH §9 社区标准配方）：
 * 挂载时 init → 卸载时 dispose（不销毁会泄漏 zrender 实例）→
 * ResizeObserver 监听容器尺寸变化触发 resize → option 变更时 setOption
 */
import { useEffect, useRef } from "react";
import * as echarts from "echarts/core";
import type { EChartsCoreOption } from "echarts/core";

export default function Chart({
  option,
  height,
}: {
  option: EChartsCoreOption;
  height: number;
}) {
  const ref = useRef<HTMLDivElement>(null);

  // 生命周期：仅挂载/卸载各执行一次
  useEffect(() => {
    const el = ref.current!;
    const chart = echarts.init(el);
    const onResize = () => chart.resize();
    const ro = new ResizeObserver(onResize);
    ro.observe(el);
    return () => {
      ro.disconnect();
      chart.dispose();
    };
  }, []);

  // 数据/配置更新：复用已有实例，不重建
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    echarts.getInstanceByDom(el)?.setOption(option);
  }, [option]);

  return <div ref={ref} style={{ width: "100%", height }} />;
}
