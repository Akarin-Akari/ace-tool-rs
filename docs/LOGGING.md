# acemcp 日志与监控

## 文件日志

启动时指定 `--log-file` 可将 tracing 日志写入文件，便于脱离 IDE 调试：

```bash
# 写入 .ace-tool 目录（默认 acemcp.yyyy-MM-dd）
ace-tool-rs --base-url ... --token ... --log-file .ace-tool

# 写入指定目录
ace-tool-rs --base-url ... --token ... --log-file C:\logs\acemcp

# 环境变量
ACE_LOG_FILE=.ace-tool ace-tool-rs ...
```

## 日志级别

- `RUST_LOG`：默认 env filter（如 `RUST_LOG=debug`）
- `--log-level` / `ACE_LOG_LEVEL`：覆盖级别（trace|debug|info|warn|error）

```bash
RUST_LOG=ace_tool=debug ace-tool-rs --log-file .ace-tool ...
```

## 独立监控脚本

不依赖 IDE，在单独终端 tail 日志：

```powershell
# 默认监控 .ace-tool 目录下最新 acemcp.* 文件
.\scripts\watch-acemcp-logs.ps1

# 指定目录
.\scripts\watch-acemcp-logs.ps1 C:\logs\acemcp

# 指定具体文件
.\scripts\watch-acemcp-logs.ps1 .ace-tool\acemcp.2025-01-31
```

## 关键日志事件

| 事件 | 级别 | 说明 |
|------|------|------|
| `mcp: tool call` | info | MCP 工具被调用 |
| `search_context: start` | info | 搜索开始 |
| `search_context: 429 rate limited, retrying` | warn | 429 限流，重试中 |
| `search_context: 5xx server error, retrying` | warn | 5xx 错误，重试中 |
| `search_context: success` | info | 搜索成功 |
| `mcp: result` | info | 工具执行结果 |

## HTTP 请求日志

设置 `ACE_HTTP_LOG=1` 可将 HTTP 请求/响应写入 `.ace-tool/http_requests.log`（与 tracing 日志分离）。
