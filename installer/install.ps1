$ErrorActionPreference = 'Stop'

$src = Split-Path -Parent $MyInvocation.MyCommand.Path
$dest = Join-Path $env:LOCALAPPDATA 'Programs\LocalToDo'
New-Item -ItemType Directory -Force -Path $dest | Out-Null

Copy-Item (Join-Path $src 'LocalToDo.exe') (Join-Path $dest 'LocalToDo.exe') -Force
Copy-Item (Join-Path $src 'uninstall.ps1') (Join-Path $dest 'uninstall.ps1') -Force

$programs = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
$lnkPath = Join-Path $programs 'Local To Do.lnk'
$ws = New-Object -ComObject WScript.Shell
$sc = $ws.CreateShortcut($lnkPath)
$sc.TargetPath = Join-Path $dest 'LocalToDo.exe'
$sc.WorkingDirectory = $dest
$sc.IconLocation = (Join-Path $dest 'LocalToDo.exe') + ',0'
$sc.Description = 'Local To Do - 本地优先的个人任务与备忘录'
$sc.Save()

$exe = Join-Path $dest 'LocalToDo.exe'
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\LocalToDo'
New-Item -Path $key -Force | Out-Null
New-ItemProperty -Path $key -Name DisplayName -Value 'Local To Do' -PropertyType String -Force | Out-Null
New-ItemProperty -Path $key -Name DisplayVersion -Value '1.0.0' -PropertyType String -Force | Out-Null
New-ItemProperty -Path $key -Name Publisher -Value 'Local To Do' -PropertyType String -Force | Out-Null
New-ItemProperty -Path $key -Name InstallLocation -Value $dest -PropertyType String -Force | Out-Null
New-ItemProperty -Path $key -Name DisplayIcon -Value $exe -PropertyType String -Force | Out-Null
$uninstall = 'powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "' + (Join-Path $dest 'uninstall.ps1') + '"'
New-ItemProperty -Path $key -Name UninstallString -Value $uninstall -PropertyType String -Force | Out-Null
New-ItemProperty -Path $key -Name NoModify -Value 1 -PropertyType DWord -Force | Out-Null
New-ItemProperty -Path $key -Name NoRepair -Value 1 -PropertyType DWord -Force | Out-Null
$size = [math]::Ceiling((Get-Item $exe).Length / 1KB)
New-ItemProperty -Path $key -Name EstimatedSize -Value $size -PropertyType DWord -Force | Out-Null

Start-Process -FilePath $exe -WorkingDirectory $dest
