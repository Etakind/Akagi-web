// 当前运行平台固定为雀魂；历史记录标签独立定义。
const MAJSOUL = { kind: 'Majsoul' as const, labelKey: 'platform.majsoul', defaultStartUrl: 'https://game.maj-soul.com/1/' }
export function platformInfo() { return MAJSOUL }
