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

REM Create test user (administrator)
net user borgtest Password1! /add
net localgroup Administrators borgtest /add

REM Create a STANDARD (non-admin) user for the non-admin edge validation.
REM Deliberately NOT added to the Administrators group.
net user borgstd Password1! /add

REM Initialize the second disk (DISK2_SIZE) as D: NTFS, for the multi-drive edge
REM validation. On an already-provisioned VM this is a no-op; run.sh re-runs an
REM idempotent equivalent over SSH for VMs recreated with the new disk.
(
  echo select disk 1
  echo online disk noerr
  echo attributes disk clear readonly noerr
  echo convert gpt noerr
  echo create partition primary noerr
  echo format fs=ntfs quick label=BORGD
  echo assign letter=D
) > "%TEMP%\initd.txt"
diskpart /s "%TEMP%\initd.txt"

echo [BorgUI Smoke] Setup complete.
