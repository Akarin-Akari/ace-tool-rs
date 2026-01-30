# watch-acemcp-logs.ps1
# 独立监控 acemcp 日志，不依赖 IDE 输出
# 用法: .\watch-acemcp-logs.ps1 [log_dir_or_file]
# 示例: .\watch-acemcp-logs.ps1 .ace-tool
#       .\watch-acemcp-logs.ps1 C:\logs\acemcp
# 环境变量: ACE_LOG_FILE 指定日志目录时，日志写入该目录下的 acemcp.yyyy-MM-dd

param(
    [string]$LogPath = ".ace-tool"
)

$ErrorActionPreference = "Stop"

function Get-LatestAcemcpLog {
    param([string]$Dir)
    if (-not (Test-Path $Dir)) {
        return $null
    }
    $files = Get-ChildItem -Path $Dir -Filter "acemcp.*" -File -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending
    if ($files) {
        return $files[0].FullName
    }
    return $null
}

# 解析路径：目录或文件
$targetPath = $LogPath
if (Test-Path $targetPath -PathType Leaf) {
    # 直接指定了日志文件
    $logFile = $targetPath
}
else {
    # 目录：找最新的 acemcp.* 文件
    $logFile = Get-LatestAcemcpLog -Dir $targetPath
    if (-not $logFile) {
        Write-Host "未找到日志文件，目录: $targetPath" -ForegroundColor Yellow
        Write-Host "请先启动 acemcp 并指定 --log-file $targetPath" -ForegroundColor Yellow
        Write-Host "示例: ace-tool-rs --base-url ... --token ... --log-file $targetPath" -ForegroundColor Cyan
        exit 1
    }
}

Write-Host "监控日志: $logFile" -ForegroundColor Green
Write-Host "按 Ctrl+C 退出" -ForegroundColor Gray
Write-Host ("=" * 60)

Get-Content -Path $logFile -Wait -Tail 50
