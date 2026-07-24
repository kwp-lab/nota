# Nota

Nota 是一个仅面向 Windows 11 x64 的本地会议录音客户端。它使用 Windows Core Audio 直接采集指定应用进程树或输出设备，同时采集麦克风，在本机完成回声消除、混音和 Ogg Opus 编码。

## 隐私与边界

- 不需要账号，不含遥测、云服务或自动更新；运行时不主动建立网络连接。
- 不包含转写、摘要、说话人识别、视频录制。
- 指定 Chrome 或 Edge 时会录制该浏览器根进程树的全部声音，不能精确到标签页。
- 首次录音时会显示隐私与告知提示，确认后不再重复打断。使用者仍应遵守所在地法律、会议规则及组织政策。
- 首版未做代码签名，Windows SmartScreen 可能显示“未知发布者”提示。

## 输出与恢复

- 输出：48 kHz、单声道、Ogg Opus、默认 64 kbps，通常约 30 MB/小时。
- 默认目录：`文档\Nota\Recordings`。
- 录制中先写入 `%LOCALAPPDATA%\Nota\Recovery`；正常停止后写入 EOS、持久化并移至目标目录。
- 从旧版升级时会迁移 `%LOCALAPPDATA%\Meeting Note` 和旧默认录音目录；用户自定义的保存目录不会改变。
- 崩溃后可恢复至最后一个校验通过的完整 Ogg 页。
- 设置和录音索引保存在本地 SQLite WAL 数据库；技术日志最多 3 × 10 MB，不记录音频内容。

## 开发与构建

需要 Rust stable、Node.js 22、Visual Studio 2022 Build Tools（Desktop development with C++）、Windows 11 SDK 和 CMake。仓库已锁定 Cargo 与 npm 依赖。

```powershell
npm ci
npm run build
npm test
.\scripts\build-windows.ps1
```

构建脚本会执行前端测试、Rust 测试、生成依赖许可证清单、构建按用户安装的 NSIS 安装包，并制作便携 ZIP。便携版仍将设置和恢复文件写入 LocalAppData。

## 快捷键和托盘

- `Ctrl+Alt+F9`：从空闲状态开始录音，或暂停/继续当前录音；首次使用时会显示一次提示。
- `Ctrl+Alt+F10`：停止并保存。
- 快捷键可在设置中修改或关闭；冲突时保存会失败，不覆盖其他应用。
- 关闭窗口会隐藏到托盘。录音中选择退出会要求停止并保存或取消。

## 代码结构

- `src/`：React/TypeScript 界面，不接触 PCM。
- `src-tauri/src/audio/wasapi.rs`：WASAPI 端点回环、进程树回环、麦克风、设备通知和重连。
- `src-tauri/src/audio/dsp.rs`：QPC 缺包校正、异步重采样、Sonora AEC3、混音和限制器。
- `src-tauri/src/audio/encoder.rs`：静态 libopus 与 Ogg 封装。
- `src-tauri/src/audio/recovery.rs`：Ogg 页校验、截断、EOS 恢复和跨磁盘安全移动。
- `src-tauri/src/controller.rs`：录音状态、Tauri IPC、托盘、快捷键和文件管理。

原有竞品调研文档已保留在仓库根目录。
