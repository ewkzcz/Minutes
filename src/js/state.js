// 全局状态。实时会议的状态独立于页面存在：切换页面不会中断录制。
export const S = {
  view: 'live',
  arg: null,
  cfg: null,          // 设置
  asrModels: [],
  llmModels: [],
  devices: [],
  templates: [],
  books: [],         // 专有词库（含绑定的模板 id）
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

/** 某模板录制时使用的词库：已启用，且绑定了该模板或未绑定任何模板（通用）。 */
export const booksFor = (tid) => S.books.filter((b) => b.enabled && (!b.template_ids.length || b.template_ids.includes(tid)));

export const isLive = () => S.live.state !== 'idle';
export const elapsed = () => {
  const l = S.live;
  if (!l.t0) return 0;
  return (l.state === 'paused' ? l.pausedAt : Date.now()) - l.t0 - l.pausedTotal;
};
