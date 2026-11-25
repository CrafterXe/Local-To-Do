$ErrorActionPreference = 'Stop'

$here = $PSScriptRoot
$root = Split-Path -Parent $here
$stage = Join-Path $here 'stage'
$sedPath = Join-Path $here 'installer.sed'
$target = Join-Path $root 'publish\LocalToDo-Setup-1.0.0.exe'

Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $stage | Out-Null
Copy-Item (Join-Path $root 'publish\LocalToDo.exe') $stage -Force
Copy-Item (Join-Path $here 'install.ps1') $stage -Force
Copy-Item (Join-Path $here 'uninstall.ps1') $stage -Force

$sed = @"
[Version]
Class=IEXPRESS
SEDVersion=3
[Options]
PackagePurpose=InstallApp
ShowInstallProgramWindow=1
HideExtractAnimation=1
UseLongFileName=1
InsideCompressed=0
CAB_FixedSize=0
CAB_ResvCodeSigning=0
RebootMode=N
InstallPrompt=
DisplayLicense=
FinishMessage=
TargetName=$target
FriendlyName=Local To Do Setup
AppLaunched=powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File install.ps1
PostInstallCmd=<None>
AdminQuietInstCmd=powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File install.ps1
UserQuietInstCmd=powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File install.ps1
SourceFiles=SourceFiles
[Strings]
FILE0="LocalToDo.exe"
FILE1="install.ps1"
FILE2="uninstall.ps1"
[SourceFiles]
SourceFiles0=$stage\
[SourceFiles0]
%FILE0%=
%FILE1%=
%FILE2%=
"@

[System.IO.File]::WriteAllText($sedPath, $sed, [System.Text.Encoding]::ASCII)

if (Test-Path $target) { Remove-Item $target -Force }
& "$env:WINDIR\System32\iexpress.exe" /N /Q $sedPath

Start-Sleep -Seconds 3
if (-not (Test-Path $target)) { throw '安装包生成失败：未找到输出文件' }
Write-Host ("安装包已生成：" + $target)
Write-Host ("大小：" + (Get-Item $target).Length + " 字节")
