# ORPHEUS

[English](README.md)

ORPHEUS 是面向 **《守望先锋 1》正式服 1.74.0.0.104319** 的开源服务端复刻项目，独立开展非官方研究与游戏保存工作。长期目标是还原该版本各英雄、地图和模式的服务端行为，以及服务端向客户端发送的事件。服务端仍在开发中，首个端到端验证场景是士兵 76 与训练靶场。

[参与贡献](CONTRIBUTING.md) · [路线图](docs/ROADMAP.md) · [Rust 分支](https://github.com/leafall903/ORPHEUS/tree/rust) · [验证记录](docs/STATE.md) · [更新日志](CHANGELOG.md) · [待处理问题](https://github.com/leafall903/ORPHEUS/issues)

## 当前进度

| 领域 | 服务端实现与测试 | 原版客户端证据与待办 |
| --- | --- | --- |
| 本地登录与大厅 | 模拟本地认证，支持大厅、英雄选择和对局交接。自动检查覆盖握手及使用已准备的运行数据启动服务端。 | 已观察到登录、训练靶场加载、选择或切换英雄。更多大厅流程及对局规则仍待验证。 |
| 士兵 76 | 已实现角色身体和武器状态、移动与疾跑、步枪射击与装填、螺旋飞弹和生物力场。自动化测试覆盖部分场景，包括反复换人及重进、丢包和弹药状态。 | 测试者曾射击并击杀训练机器人。武器显示的稳定性、弹药界面、输入响应和技能行为仍待完整实机验证。 |
| 训练靶场战斗 | 已实现命中与伤害判定、机器人生命、死亡和重生，并通过网络同步原版靶场机器人的头顶血条显示图。自动化测试使用受控测试数据及目标地图的碰撞数据。 | 测试者反馈头顶血条缺失、消灭提示无目标名、右上击杀栏缺失和爆头音效缺失。血条显示图的改动已取得报文证据，实际渲染仍待记录实机结果。 |
| 网络与研究 | 已实现加密 UDP、玩家输入、实体生命周期、服务端状态快照和回放，并开展了部分丢包和乱序检查。 | 客户端预测、战斗反馈、双端一致性、长时间运行和更多地图的行为仍待验证。 |
| 英雄、模式与音频 | 已具备共用机制，并覆盖部分功能。 | 完整英雄技能、地图目标、对局流程、命中与连杀反馈和情境语音仍在开发中。 |

自动化结果适用于证据中列出的测试场景或回放。手动观察适用于实际使用的版本、提交和操作步骤。首个目标场景的完整原版客户端验收仍待完成。详见[验证记录](docs/STATE.md)和[更新日志](CHANGELOG.md)。

## 如何参与

- **开发者和使用 AI 工具的贡献者**可以从公开源码测试入手。贡献指南涵盖 Codex、Claude Code，并为所有提交规定统一的审查要求。[贡献指南](CONTRIBUTING.md)
- **逆向研究者**可以帮助确认 104319 版本中的单个字段、状态切换、事件或时序规则。请从相关证据出发，并明确说明尚不确定的部分。[研究证据](docs/RESEARCH_EVIDENCE.txt)
- **玩家**可使用目标版本客户端和已获授权的运行数据，提交简短且可复现的游玩结果。欢迎同时报告正常行为和缺陷。[游玩验证表单](https://github.com/leafall903/ORPHEUS/issues/new?template=gameplay-verification.yml)
- **初次参与者**可以查找只需公开源码和合成数据即可完成的任务。[适合入门的问题](https://github.com/leafall903/ORPHEUS/labels/good%20first%20issue)

## 在本地运行

`main` 是由社区维护的 Python 服务端与 Windows 启动器分支。Rust 原生服务端在独立的 `rust` 分支继续开发。

### 公开源码检查

文档以 Windows 和 64 位 Python 3.12 为运行环境。以下源码测试使用合成数据。单独的管理面板测试使用 Node.js 24。

```powershell
py -3.12 -m venv .venv
.\.venv\Scripts\python.exe -m pip install -r requirements.txt "ruff==0.17.3"
.\.venv\Scripts\python.exe -B -m unittest discover -s tests -p test_game_commands.py
```

扩大测试范围之前，请查看[完整的公开源码检查清单](CONTRIBUTING.md#public-source-checks)。

### 独立的部署数据

本地游玩需要 **1.74.0.0.104319** 游戏安装目录，以及已获授权并准备好的服务端运行数据。Python 服务端使用其中的协议数据、签名所需数据、玩法状态脚本图和地图碰撞缓存。仅凭游戏目录即可完成安装是后续交付目标。所需文件及现有准备步骤见[运行数据指南](data/README.md)。

将 `ORPHEUS_RUNTIME_DIR` 指向准备好的数据包。启动预检会列出缺失的启动输入并检查签名输入的长度，可以在安装 Python 依赖前运行。地图碰撞缓存会在加载场景时另行检查。

```powershell
$env:ORPHEUS_RUNTIME_DIR = 'C:\path\to\orpheus-runtime'
py -3.12 -B -m ow174 --check-runtime
```

如果本地研究目录已有所需输入，可用 Python 打包工具将文件复制到新的目录并写入清单。靶场使用 `--map 0688`，省略该参数可复制所有已准备好的地图缓存。

```powershell
py -3.12 -B tools/prepare_server_runtime.py --source 'C:\path\to\local-research' --destination 'C:\path\to\orpheus-runtime' --map 0688
```

### 启动纯服务端或本地游玩

运行纯服务端前先设置 `ORPHEUS_RUNTIME_DIR`，然后执行 `START_SERVER_LOCAL.cmd`。它会启动本地回环服务，管理面板地址是 `http://127.0.0.1:3725`。这一入口只需要已准备好的运行数据和 Python 环境。

本地游玩需要 Windows 中继。已有可信的本地编译产物 `relay/owwfd_relay.dll` 时可以复用。首次编译请安装 x64 MSVC Build Tools，打开 x64 Native Tools 命令提示符，在仓库根目录执行。

```bat
.\relay\build.bat
```

配置运行数据并编译中继后，把游戏安装目录或其中的 `_retail_\Overwatch.exe` 传给本地启动器。省略路径参数时，启动器会使用已保存的客户端路径或打开文件选择框。

```powershell
.\START_LOCAL.cmd --game-exe 'C:\path\to\Overwatch'
```

启动器会在需要时创建 Python 环境，并保存所选安装目录，供下次启动和准备碰撞数据使用。测试记录应包含的信息见[原版客户端验证步骤](CONTRIBUTING.md#original-client-verification)。

## 项目结构

| 路径 | 职责 |
| --- | --- |
| `ow174/bnet`、`ow174/lobby` | 本地登录、账号、社交和大厅服务、匹配与对局交接 |
| `ow174/game`、`ow174/game/script` | 游戏加密通信、输入、模拟、战斗及状态脚本同步 |
| `ow174/dashboard`、`relay` | 本地管理和 Windows 客户端中继源码 |
| `tests`、`tools`、`docs` | 验证、提取、回放工具及研究记录 |

[路线图](docs/ROADMAP.md)跟踪整个目标版本的还原范围。[致谢](CREDITS.md)记录了原始 1.74 大厅项目、研究来源和上游作者。公开源码保留原有的 [MIT 许可证声明](LICENSE)。《守望先锋》及其资源归各自权利人所有。请将游戏客户端、商业游戏资源、密钥、私有日志、原始抓包和转储留在本地。
