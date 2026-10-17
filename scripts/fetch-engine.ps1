# Скачивает движки обхода в src-tauri/resources/engine:
#   z1/    — winws (zapret1) + стратегии и фейки Flowseal/zapret-discord-youtube
#   z2/    — winws2 (zapret2) + lua + фильтры windivert + фейки
#   lists/ — списки доменов и IP (общие для обоих движков)
#
# Windows Defender помечает winws как HackTool. Если распаковка падает —
# добавьте исключение на папку проекта (Add-MpPreference -ExclusionPath ...).

param(
    [string]$Out = (Join-Path $PSScriptRoot "..\src-tauri\resources\engine")
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

function Get-LatestRelease($repo) {
    Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" -Headers @{ "User-Agent" = "prosvet-build" }
}

$tmp = Join-Path $PSScriptRoot "..\.engine-tmp"; if (Test-Path $tmp) { Remove-Item -Recurse -Force $tmp }
New-Item -ItemType Directory -Force $tmp | Out-Null
$Out = [IO.Path]::GetFullPath($Out)
if (Test-Path $Out) { Remove-Item -Recurse -Force $Out }
$z1 = Join-Path $Out "z1"; $z2 = Join-Path $Out "z2"; $lists = Join-Path $Out "lists"
New-Item -ItemType Directory -Force $z1, (Join-Path $z1 "strategies"), $z2, $lists | Out-Null

# --- Flowseal ---
$fs = Get-LatestRelease "Flowseal/zapret-discord-youtube"
$fsAsset = $fs.assets | Where-Object { $_.name -like "*.zip" } | Select-Object -First 1
Write-Host "Flowseal $($fs.tag_name)"
Invoke-WebRequest $fsAsset.browser_download_url -OutFile "$tmp\fs.zip"
Expand-Archive "$tmp\fs.zip" "$tmp\fs" -Force
$fsRoot = Get-ChildItem "$tmp\fs" -Recurse -Filter "service.bat" | Select-Object -First 1 | ForEach-Object { $_.DirectoryName }
Copy-Item "$fsRoot\bin\*" $z1 -Recurse
Get-ChildItem $fsRoot -Filter "general*.bat" | Copy-Item -Destination (Join-Path $z1 "strategies")
Copy-Item "$fsRoot\lists\*" $lists
Copy-Item "$fsRoot\utils\targets.txt" $Out -ErrorAction SilentlyContinue

# --- zapret2 ---
$z2r = Get-LatestRelease "bol-van/zapret2"
$z2Asset = $z2r.assets | Where-Object { $_.name -like "zapret2-v*.zip" } | Select-Object -First 1
Write-Host "zapret2 $($z2r.tag_name)"
Invoke-WebRequest $z2Asset.browser_download_url -OutFile "$tmp\z2.zip"
Expand-Archive "$tmp\z2.zip" "$tmp\z2" -Force
$z2Root = Get-ChildItem "$tmp\z2" -Directory | Select-Object -First 1 | ForEach-Object { $_.FullName }
Copy-Item "$z2Root\binaries\windows-x86_64\*" $z2
New-Item -ItemType Directory -Force "$z2\lua", "$z2\filter", "$z2\fake" | Out-Null
Copy-Item "$z2Root\lua\*.lua" "$z2\lua"
Copy-Item "$z2Root\init.d\windivert.filter.examples\*" "$z2\filter"
Copy-Item "$z2Root\files\fake\*" "$z2\fake"
# фейки Flowseal тоже пригодятся стратегиям zapret2
Copy-Item "$z1\*.bin" "$z2\fake" -Force

@{ flowseal = $fs.tag_name; zapret2 = $z2r.tag_name } | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $Out "version.json")
Remove-Item -Recurse -Force $tmp
Write-Host "OK -> $Out"
