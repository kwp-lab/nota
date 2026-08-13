# Contributing to Nota / 参与 Nota 贡献

Thank you for helping improve Nota. Bug fixes, focused features, tests,
documentation, accessibility improvements, and Windows compatibility reports
are welcome.

感谢你帮助改进 Nota。我们欢迎缺陷修复、边界清晰的功能、测试、文档、无障碍
改进，以及 Windows 设备和会议软件兼容性反馈。

## Before you start / 开始之前

- Search [existing issues](https://github.com/kwp-lab/nota/issues) before filing
  a new report. Open an issue before a large feature or architectural change so
  its scope can be agreed before implementation.
- Never attach real meeting audio, transcripts, generated documents, API keys,
  logs containing private data, or identifiable local paths. Use synthetic or
  thoroughly anonymized fixtures.
- Read [`AGENTS.md`](AGENTS.md) for repository-wide invariants and
  [`docs/README.md`](docs/README.md) for the owning technical specifications.
- Keep a pull request focused. Do not include generated `dist/`, `release/`,
  `artifacts/`, `node_modules/`, or `src-tauri/target/` output.

- 提交新问题前请先搜索[现有 Issues](https://github.com/kwp-lab/nota/issues)。
  对大型功能或架构调整，请先创建 Issue，就范围达成一致后再实施。
- 不得附带真实会议录音、转写正文、AI 文档、API Key、包含隐私内容的日志，
  或可识别用户身份的本地路径。测试材料应为合成数据或彻底匿名化数据。
- 请先阅读 [`AGENTS.md`](AGENTS.md) 中的全局约束，以及
  [`docs/README.md`](docs/README.md) 索引的技术规范。
- 一个 PR 只处理一个主题，不要提交 `dist/`、`release/`、`artifacts/`、
  `node_modules/` 或 `src-tauri/target/` 等生成目录。

## Architecture boundaries / 架构边界

- Recording must remain fully functional offline. Network requests happen only
  for explicit or user-enabled transcription and AI-generation actions.
- Raw audio, SQLite and filesystem access, provider credentials, and ASR/LLM
  HTTP requests belong in Rust. React receives typed state and events.
- Preserve the selected capture scope. A capture failure must be visible and
  must not silently fall back to system-wide audio.
- Do not introduce telemetry, cloud services, automatic updates, runtime DLLs,
  virtual audio drivers, or FFmpeg without prior maintainer agreement.
- Frontend work must follow [`docs/design-system.md`](docs/design-system.md)
  and use the shared design tokens and components.

- 录音必须保持完全离线可用。只有用户明确发起或启用自动转写、AI 生成时才可联网。
- 原始音频、SQLite 与文件系统访问、Provider 凭据以及 ASR/LLM HTTP 请求归 Rust
  后端负责；React 只接收类型化状态和事件。
- 必须保留用户选择的捕获范围。捕获失败应明确提示，不得静默回退到全系统录音。
- 未经维护者事先确认，不要引入遥测、云服务、自动更新、运行时 DLL、虚拟声卡或
  FFmpeg。
- 前端改动必须遵循 [`docs/design-system.md`](docs/design-system.md)，复用统一的
  Design Token 和共享组件。

## Development workflow / 开发流程

1. Fork the repository or create a focused branch from the latest `main`.
2. Install the locked dependencies with `npm ci` on Windows 11 x64. The full
   prerequisites are documented in [Build from source](README.md#build-from-source).
3. Add or update tests for behavior changes. Update the owning specification
   and `CHANGELOG.md` when behavior visible to users changes.
4. Run checks appropriate to the change:

   | Change | Required local checks |
   |---|---|
   | React or TypeScript | `npm test` and `npm run build:web` |
   | Visual styling | Above, plus `npm run check:design` |
   | Rust or Tauri | Rust formatting, locked tests, and Clippy |
   | Cross-layer, dependency, version, or release | `npm run check` |

5. Open a pull request that explains the problem, the chosen behavior, test
   evidence, and any user-visible or compatibility impact.

1. Fork 仓库，或从最新 `main` 创建一个主题明确的分支。
2. 在 Windows 11 x64 上运行 `npm ci` 安装锁定依赖。完整环境要求见
   [从源码构建](README.zh-CN.md#从源码构建)。
3. 行为变化必须补充或更新测试；同时更新对应技术规范，用户可见变化还应更新
   `CHANGELOG.md`。
4. 按上表运行与改动范围对应的检查。跨前后端、依赖、版本或发布改动应运行完整的
   `npm run check`。
5. 创建 PR，说明要解决的问题、最终行为、测试依据，以及用户体验或兼容性影响。

Normal pull requests do not need to build `Nota.exe`, an NSIS installer, or a
portable release. Those artifacts are produced only by the release workflow.

普通 PR 无需构建 `Nota.exe`、NSIS 安装包或便携包；这些产物只由正式发布流程生成。

## License / 许可证

By contributing, you agree that your contribution may be distributed under
the repository's [MIT License](LICENSE). Third-party code or assets must retain
their notices and have a license compatible with distribution by Nota.

提交贡献即表示你同意该贡献可依据本仓库的 [MIT License](LICENSE) 分发。引入第三方
代码或资源时，必须保留其许可证声明，并确保其许可证允许 Nota 进行分发。
