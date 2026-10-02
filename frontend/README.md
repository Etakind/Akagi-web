# 前端开发

Tauri 桌面界面使用 React、TypeScript、Vite、Zustand 和 i18next。界面语言继续保留多语言；项目使用说明只维护根目录中文 README。

```sh
npm ci
npm test
npm run lint
npm run build
```

`types.ts` 与 Rust schema/config 保持一致；`lib/tauri.ts` 调用命令，`hooks/useTauriBridge.ts` 订阅事件。
`stores/` 保存界面状态，`routes/Setup.tsx` 和 `routes/Settings.tsx` 只配置雀魂浏览器采集，端口 0 为独立模式，非零为回环附加模式。
`capture.enabled` 是采集启动开关，`unavailable_reason` 显示旧配置被停用的原因。

`tiles/CaptureControlTile.tsx` 保留历史布局 ID `proxy-control`，仅为已保存布局兼容，界面和运行功能均为浏览器采集。
历史平台标签、房间显示和旧 Inspector text/binary/mitm 来源继续兼容读取；不提供其他游戏运行入口或 PT 规则。

Inspector 收到的帧已脱敏，应显示 redacted 原因；不得在前端恢复原始帧或请求正文。
界面测试和生产构建不代表 macOS/Windows 浏览器实机验收。前端通用 CI 可在 Linux 运行。
