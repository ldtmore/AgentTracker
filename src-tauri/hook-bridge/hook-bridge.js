// AgentTrackerIsland hook 桥:Claude Code hook → 本地事件文件(零依赖,单文件)
// 安装后 command 形如:node "<home>/.claude/hooks/hook-bridge.js"(不需要事件名参数,
// 事件名从 stdin 的 hook_event_name 读取——实测字段,2026-09-16)
//
// 设计约束(红线②故障隔离):只 append 本地文件即退出,不连端口、不找主进程;
// AgentTrackerIsland 未运行时本脚本依旧秒级成功,Claude Code 零感知。
const fs = require('fs');
const path = require('path');
const os = require('os');

const eventsFile = path.join(
  process.env.LOCALAPPDATA || path.join(os.homedir(), 'AppData', 'Local'),
  'AgentTrackerIsland', 'events', 'claude-code.jsonl'
);

let buf = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (c) => { buf += c; });
process.stdin.on('end', () => { writeEvent(buf); process.exit(0); });
// stdin 异常不结束时 2 秒兜底退出,绝不阻塞 Claude Code
setTimeout(() => { writeEvent(buf); process.exit(0); }, 2000).unref();

// 白名单提取字段写入事件行(不落对话内容,隐私最小化)
function writeEvent(raw) {
  try {
    const j = JSON.parse(raw || '{}');
    const pick = (k) => (typeof j[k] === 'string' && j[k] ? j[k] : undefined);
    const event = {
      ts: Date.now(),
      hook: pick('hook_event_name') || 'unknown',
      session_id: pick('session_id') || '',
      // 工具事件(PreToolUse/PostToolUse)与通知事件的专有字段
      tool_name: pick('tool_name'),
      // Notification 的消息文本(额度/限流关键词判定用,T7 状态聚合)
      message: pick('message'),
    };
    fs.mkdirSync(path.dirname(eventsFile), { recursive: true });
    fs.appendFileSync(eventsFile, JSON.stringify(event) + '\n');
  } catch (e) { /* 静默失败:诊断信息写向 stderr 会污染 Claude Code */ }
}
