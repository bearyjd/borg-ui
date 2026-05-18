@echo off
REM First-boot setup script — runs automatically via dockurr/windows OEM hook

echo [BorgUI Smoke] Installing OpenSSH Server...
powershell -Command "Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0"
powershell -Command "Start-Service sshd"
powershell -Command "Set-Service -Name sshd -StartupType Automatic"

REM Allow SSH through firewall
netsh advfirewall firewall add rule name="OpenSSH" dir=in action=allow protocol=TCP localport=22

REM Set default shell to PowerShell for SSH sessions
reg add "HKLM\SOFTWARE\OpenSSH" /v DefaultShell /t REG_SZ /d "C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe" /f

REM Create test user
net user borgtest Password1! /add
net localgroup Administrators borgtest /add

echo [BorgUI Smoke] Setup complete.
