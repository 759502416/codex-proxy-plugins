# 额度重置卡

批量或按额度阈值自动使用 OpenAI OAuth 账号的主动额度重置卡。插件通过宿主原生管理接口完成查卡、扣卡和额度回读，不直接访问上游，也不保存管理员凭据

| 项 | 值 |
| --- | --- |
| 插件 ID | `wanghuxiong.reset-credits` |
| 宿主版本 | `>=3.21.0, <4.0.0`，SDK 与打包 CLI 固定到宿主 v3.21.3（`cb397ae5`） |
| 能力 | `management` 与 `maintenance`，无配置项，安装后直接启用 |
| 私有状态 | `accounts` 每个账号的操作记录，`automation` 每个账号的自动用卡记录，`settings` 全部账号的自动用卡设置 |

## 使用

安装后从左侧「插件 → 额度重置卡」打开页面：

1. 列表只显示宿主标记了重置卡能力的账号，API Key 账号和 xAI 账号不出现
2. 勾选账号后点「查询重置卡」，实时查询剩余张数和最早到期时间；列表本身不自动查卡
3. 点「批量重置」时先补齐卡数，确认弹窗列出可重置数量和跳过原因（无可用卡、查卡失败、待确认、不支持消费）
4. 确认后每个账号消耗 1 张最早到期的卡，成功后重新查卡并刷新额度，账号是否恢复由宿主按上游额度判断

宿主账号页原有的单账号重置仍可使用，两处调用同一个原生接口

### 自动用卡

勾选账号后点「自动用卡」设置周额度阈值，可选同时按 5 小时额度触发；未设置的账号不会自动扣卡。宿主每 30 秒调用一次维护入口：

1. 用宿主已有的额度观测粗筛，只看 Codex 主额度，Spark 等附加额度不参与；未达到阈值的账号不访问上游
2. 达到阈值时先从上游刷新额度复核，再读一次设置确认开关仍然开启
3. 以周额度的重置时间标识周期，每个周期最多自动用一张卡；先记录周期占用再扣卡，同一周期不会扣第二张
4. 扣卡流程与手动重置相同，结果未知时下一轮用同一请求标识确认
5. 没有卡、上游未执行或查询失败时释放周期占用并退避，失败 2 分钟、其余 10 分钟内不再为该账号访问上游

每轮最多用 12 秒开始处理新账号，来不及处理的账号下一轮继续。表格「自动用卡」列显示阈值和最近一次自动处理结果

## 扣卡安全

扣卡不可逆，插件按以下规则避免重复扣卡：

- 发出消费前先在私有状态写入幂等键和卡片 ID，写入按版本校验，同一账号同时只允许一笔消费
- 宿主报告结果未知（`50202`）、调用中断或超时时保留记录，账号显示为「待确认」，不能发起新的重置
- 「确认上次重置」原样复用记录中的幂等键和卡片 ID，上游返回 `already_redeemed` 视为上次已完成
- 首次发出被宿主明确拒绝时清除记录；重试被拒时记录继续保留，因为首次消费的结果仍然未知
- 扣卡成功后的回读失败只作为提醒返回，不报告为失败
- 「放弃」只删除待确认记录，上次可能已经扣卡，放弃后再重置可能多扣一张
- 自动用卡也会确认手动留下的待确认记录，但只在该账号开启自动用卡且额度达到阈值时进行

页面不接收卡片 ID 和幂等键，日志只记录账号 ID 与结果码

## 开发与打包

```text
plugin.json       插件身份、能力、资源与私有状态声明
backend/          Rust 处理器与会话测试
frontend/         Vue 页面与独立预览
scripts/package   构建 Linux 安装包
```

| 想看什么 | 入口 |
| --- | --- |
| 调用宿主管理接口 | [`host_admin.rs`](backend/src/host_admin.rs)，使用 `HostClient::dispatch_http` |
| 重置流程与结果判定 | [`reset.rs`](backend/src/reset.rs)，手动与自动共用 |
| 选卡与卡数汇总 | [`credits.rs`](backend/src/credits.rs) |
| 自动用卡判定与周期 | [`automation/runner.rs`](backend/src/automation/runner.rs)、[`windows.rs`](backend/src/automation/windows.rs) |
| 私有状态读写 | [`state.rs`](backend/src/state.rs)、[`operations.rs`](backend/src/operations.rs) |
| 批量队列与页面状态 | [`useResetCredits.ts`](frontend/src/composables/useResetCredits.ts) |

后端检查与测试需要 Rust 1.97，本机没有 Rust 时可在 `rust:1.97-bookworm` 容器中执行：

```bash
cargo fmt --manifest-path plugins/reset-credits/backend/Cargo.toml --check
RUST_MIN_STACK=16777216 cargo clippy --manifest-path plugins/reset-credits/backend/Cargo.toml --all-targets --all-features --locked -- -D warnings
RUST_MIN_STACK=16777216 cargo test --manifest-path plugins/reset-credits/backend/Cargo.toml --locked
```

页面检查与本地预览，预览使用模拟数据，不访问宿主：

```bash
corepack pnpm --dir plugins/reset-credits/frontend install --frozen-lockfile
corepack pnpm --dir plugins/reset-credits/frontend lint
corepack pnpm --dir plugins/reset-credits/frontend dev
```

打包在 Git Bash 与 Docker Desktop 中运行，页面在本机构建，二进制和打包 CLI 在 Docker 中构建，产物写入仓库根目录的 `dist/`：

```bash
bash plugins/reset-credits/scripts/package
```

默认目标为 `x86_64-unknown-linux-gnu`，传入 `aarch64-unknown-linux-gnu` 时需要 Docker 支持 arm64 模拟。修改源码后需重新打包，并在宿主中切换到新版本
