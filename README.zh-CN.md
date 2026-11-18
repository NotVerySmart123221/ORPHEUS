# ORPHEUS (Rust)

[English](README.md) · [Discord](https://discord.gg/tqukqjAk7) · [Python 参考快照](https://github.com/leafall903/ORPHEUS/tree/45044d0d62399d9eb876c08e788aaea1f56cecc9)

ORPHEUS 是面向 **《守望先锋 1》1.74.0.0.104319** 版本的独立开源服务端复刻与协议研究项目。

**分支于 2026 年 11 月 19 日 12:00 UTC（赫尔辛基 14:00）切换。** Rust 开发从 `rust` 迁至默认分支 `main`。原 Python `main` 改名为 `python-archive`，保留作参考。从切换时起，Python 停止维护，面向归档分支的新 PR 将关闭。原生服务端目前是开发预览版，详见[分支切换清单](docs/BRANCH_TRANSITION.md)和下方的玩法验证进度。

[运行公开 Rust 测试](#运行公开-rust-测试无需游戏本体) · [启动原生服务端](#启动原生服务端) · [当前进度](#当前进度) · [服务端架构](docs/NATIVE_SERVER_ARCHITECTURE.md) · [贡献指南](CONTRIBUTING.md) · [问题列表](https://github.com/leafall903/ORPHEUS/issues)

---

## 运行公开 Rust 测试（无需游戏本体）

安装 **Git** 与 **稳定版 Rust**（`>= 1.85.0`）。在 Windows 上还需安装 **x64 MSVC C++ Build Tools** 与 **Windows SDK**，以编译内置的 Lua 5.4 运行时。

```powershell
git clone https://github.com/leafall903/ORPHEUS.git
cd ORPHEUS
if (-not (Test-Path Cargo.toml)) { git switch rust }
cargo test --locked --workspace --all-targets
```

公开测试套全部基于合成 JSON 数据运行。Clippy、CI 校验命令与 38 个 StateScript 共享契约跑法详见 [CONTRIBUTING.md](CONTRIBUTING.md)。

---

## 启动原生服务端

启动原生服务端需要先准备一份**基础 104319 靶场运行包**（可选包含怪鼠复仇数据），文件清单与打包步骤见 [data/README.md](data/README.md)。

```powershell
$env:ORPHEUS_RUNTIME_DIR = 'C:\path\to\authorized-base-runtime'
cargo run --locked --bin orpheus_server -- --check-runtime
cargo run --locked --bin orpheus_server -- --data-dir 'C:\path\to\local-state'
```

开启本地只读 3D 调试后台（`--features inspector`）：

```powershell
cargo run --locked --features inspector --bin orpheus_server -- --data-dir 'C:\path\to\local-state' --debug-port 3726
```

启动后访问 `http://127.0.0.1:3726/`，即可查看房间内实时实体、碰撞网格、Bot 巡逻裁切点、Tick 耗时统计以及房间异常故障隔离记录。

### 本地怪鼠复仇验收配置（普通难度卡西迪）

仅在本地做普通难度复仇（`0x3C`）验收测试时，可通过 `--junkenstein-validation <path.json>` 加载本地规则覆盖：

- **城门超高血量胜利测试**：`{"invulnerable_cassidy": true, "gate_capacity": 1000000000}`
- **可摧毁城门失败测试**：`{"invulnerable_cassidy": true}`

该配置仅对 `0x3C` 卡牌生效，不影响默认启动或其它英雄/难度，测试配置文件请保存在仓库外。

---

## 当前进度

| 模块 | Rust 服务端现状 | 104319 原版客户端情况与待办 |
| --- | --- | --- |
| **登录、存档与画廊** | 原生 Battle.net / JAM 大厅、玩家设置持久化（`profile-settings-v1`）、饰品与 23 个 NPC 英雄画廊预览策略。 | 登录与靶场进图已跑通；最新 NPC 画廊预览与完整实机流程待复测。 |
| **靶场与士兵 76** | 加密 UDP 房间、Bevy ECS Bot/投射物管理、Lua 巡逻策略、步枪/换弹/疾跑/螺旋飞弹/生物力场仿真与靶场重生。 | 无客户端加密回环全部通过；客户端武器挂载、HUD 同步与二楼巡逻细节待实机复测。 |
| **怪鼠复仇与卡西迪** | 全部 14 张卡牌初始化完成；普通难度卡西迪左/右键、翻滚、闪光弹、午时已到、`0B8B` 充能、计分、Boss 语音与城门阶段已通过离线回放。 | 已实机确认失败结算标题；卡西迪全部技能手感、Boss 登场镜头/追击、结算耗时与自然胜利待实机验收。 |
| **底层架构与隔离** | `orpheus-wire`、`orpheus-collision`、`orpheus-runtime` 已独立成 Crate；`script_init/`、`game_combat/`、`scene_assets/`、`game_service/` 完成模块化拆分及房间级 Panic 隔离。 | 剩余大文件拆分（#60）、双向契约补齐（#61）、3D Inspector 增强（#62）、通用 NavMesh（#63）、独立 `statescript` Crate（#64）与全 ECS 统一（#65）已建 Issue 跟进。 |

更多详情见 [架构与迁移](docs/NATIVE_SERVER_ARCHITECTURE.md) · [进度与对等状态](docs/STATE.md) · [行为契约](docs/BEHAVIOR_CONTRACTS.md) · [路线图](docs/ROADMAP.md)。

---

## 仓库目录结构

| 路径 | 职责 |
| --- | --- |
| `rust/src/` | 主服务端（`orpheus_server`）、大厅与匹配、同步投递、ECS 战斗模拟（`game_combat/`）、场景加载（`scene_assets/`）、StateScript 虚拟机（`script_init/`） |
| `rust/crates/wire/`（`orpheus-wire`） | JAM 加密流、分帧协议、编解码与输入命令报文 |
| `rust/crates/collision/`（`orpheus-collision`） | 碰撞缓存解码、空间三角索引、射线查询与 GJK/EPA 接触求解器 |
| `rust/crates/runtime/`（`orpheus-runtime`） | 运行时清单校验、`--check-runtime` 自检与 `prepare_server_runtime` 打包工具 |
| `rust/scripts/` | Lua 巡逻策略脚本（`patrol.lua`） |
| `rust/assets/inspector/`、`rust/tests/` | 3D 调试前端页面与跨模块集成测试（`world_queries`、`profile_settings`、`protocol`、`runtime_preflight`） |
| `fixtures/behavior/`、`scripts/` | 48 个 JSON 行为夹具（包含仅用于 Rust 的回归契约）及跨语言校验脚本 |
| `relay/` | Windows C++ 客户端 TLS 中继源码 |

## 许可与声明

源码采用 [MIT 协议](LICENSE)，贡献者与致谢见 [CREDITS.md](CREDITS.md)。严禁向公开仓库提交游戏客户端、商业资源、签名输入、密钥、账号数据或原始网络抓包。
