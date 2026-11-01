# AIChat v0.30.0 简体中文版（Windows x64）

这是基于 AIChat v0.30.0 的个人简体中文本地化版本。命令名称、配置键、模型 ID、API 参数和协议字段保持英文，以确保兼容性。

## Windows 便携版

将以下文件放在同一个文件夹中：

- `aichat.exe`
- `启动中文版.bat`

双击 `启动中文版.bat` 即可运行。首次启动时，程序会在同目录创建 `data` 文件夹，并将配置、角色、会话等数据保存在其中，不会默认写入 Windows 用户的 AppData 目录。

推荐目录结构：

```text
aichat/
├── aichat.exe
├── 启动中文版.bat
└── data/
    ├── config.yaml
    ├── sessions/
    ├── roles/
    └── ...
```

请备份整个 `data` 文件夹，它包含 API 配置和聊天会话。注意：其中的 API Key 属于敏感信息，不要上传到公开仓库或分享给他人。

## 命令

在交互界面输入 `.help` 查看中文帮助。命令本身保持英文，例如：

- `.session <名称>`：创建或切换会话
- `.save session`：保存当前会话
- `.model`：切换模型
- `.edit config`：编辑配置文件
- `.exit`：退出

## 命令行参数

运行 `aichat.exe --help` 查看中文参数说明。参数名称仍为英文，例如 `--model`、`--session`、`--serve`。

## 从旧版迁移

如果此前使用过官方版本，旧配置通常位于 Windows 用户配置目录下的 `aichat` 文件夹。请先退出程序，再将需要保留的 `config.yaml`、`sessions`、`roles` 等内容复制到本程序旁的 `data` 文件夹。不要覆盖已有文件前忘记备份。

## 构建

仓库的 GitHub Actions 会在推送到 `zh-cn-v0.30.0` 分支后自动编译 Windows x64 版本，并上传为 workflow artifact。构建产物保留 14 天。
