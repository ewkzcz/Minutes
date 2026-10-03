// 全局状态。实时会议的状态独立于页面存在：切换页面不会中断录制。
export const S = {
  view: 'live',
  arg: null,
  cfg: null,          // 设置
  asrModels: [],
  llmModels: [],
  devices: [],
  templates: [],
  sessionCount: 0,
  live: {
    state: 'idle',    // idle | recording | paused | reconnecting | finalizing
    sid: null, title: '', segs: [], stages: [], partial: '', mode: 'text',
    t0: 0, pausedAt: 0, pausedTotal: 0, interval: 5, summarizing: false, model: '',
    levels: new Array(36).fill(0),
  },
  go: () => {},        // 由 main.js 注入
  refreshNav: () => {},
};

export const isLive = () => S.live.state !== 'idle';
export const elapsed = () => {
  const l = S.live;
  if (!l.t0) return 0;
  return (l.state === 'paused' ? l.pausedAt : Date.now()) - l.t0 - l.pausedTotal;
};
