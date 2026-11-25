param([switch]$Silent)

$ErrorActionPreference = 'SilentlyContinue'
$dest = Split-Path -Parent $MyInvocation.MyCommand.Path

Get-Process LocalToDo | Stop-Process -Force
Start-Sleep -Milliseconds 800

# 清理开机自启项
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$props = Get-ItemProperty $runKey
foreach ($p in $props.PSObject.Properties) {
    if ($p.Name -like 'PS*') { continue }
    if ($p.Value -is [string] -and $p.Value -like ('*' + $dest + '*')) {
        Remove-ItemProperty -Path $runKey -Name $p.Name -Force
    }
}

# 清理开始菜单快捷方式
$lnk = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Local To Do.lnk'
Remove-Item $lnk -Force

# 询问是否删除数据
$removeData = $false
if (-not $Silent) {
    Add-Type -AssemblyName System.Windows.Forms
    $answer = [System.Windows.Forms.MessageBox]::Show(
        "是否同时删除本地数据（任务、笔记、备份、设置）？`n`n选择『否』将保留数据，方便以后重新安装。",
        '卸载 Local To Do',
        [System.Windows.Forms.MessageBoxButtons]::YesNo,
        [System.Windows.Forms.MessageBoxIcon]::Question)
    if ($answer -eq [System.Windows.Forms.DialogResult]::Yes) { $removeData = $true }
}
if ($removeData) {
    Remove-Item (Join-Path $env:APPDATA 'com.localtodo.desktop') -Recurse -Force
}

# 清理“应用和功能”中的卸载项
Remove-Item 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\LocalToDo' -Recurse -Force

# 延迟删除安装目录（脚本自身位于该目录内）
Start-Process -FilePath 'cmd.exe' -ArgumentList ('/c timeout /t 2 >nul & rmdir /s /q "' + $dest + '"') -WindowStyle Hidden
