#Requires -RunAsAdministrator
param(
    [Parameter(Mandatory = $true)][string]$Program,
    [Parameter(Mandatory = $true)][ValidateRange(1,65535)][int]$Port
)
$ErrorActionPreference = 'Stop'
$appPath = (Resolve-Path -LiteralPath $Program).Path
if ([IO.Path]::GetFileName($appPath) -ine 'SS2-Netplay.exe') { throw 'Select the SS2-Netplay.exe you are hosting from.' }
$policy = New-Object -ComObject HNetCfg.FwPolicy2
$rules = @($policy.Rules | Where-Object {
    $_.ApplicationName -ieq $appPath -and $_.Enabled -and
    $_.Direction -eq 1 -and $_.Action -eq 0 -and
    $_.Protocol -eq 17 -and $_.Profiles -eq 4
})
# Preserve each application's explicit block on every OTHER UDP port.
$blockedPorts = @()
if ($Port -gt 1) { $blockedPorts += "1-$($Port - 1)" }
if ($Port -lt 65535) { $blockedPorts += "$($Port + 1)-65535" }
$backupDir = Join-Path $PSScriptRoot 'firewall-backups'
New-Item -ItemType Directory -Path $backupDir -Force | Out-Null
$backup = Join-Path $backupDir ('before-' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff') + '.json')
@($rules | Select-Object Name,ApplicationName,Enabled,Direction,Action,Profiles,Protocol,LocalPorts) |
    ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $backup
foreach ($rule in $rules) {
    $rule.LocalPorts = $blockedPorts -join ','
}
$name = "SS2 Netplay UDP $Port - $appPath"
$allow = New-Object -ComObject HNetCfg.FWRule
$allow.Name = $name
$allow.Description = 'Allow this SS2 harness on its manually selected VPN/host port; Public profile only.'
$allow.ApplicationName = $appPath
$allow.Protocol = 17
$allow.LocalPorts = [string]$Port
$allow.Direction = 1
$allow.Profiles = 4
$allow.Action = 1
$allow.Enabled = $true
$policy.Rules.Add($allow)
Write-Output "Allowed inbound UDP $Port on Public networks for $appPath"
Write-Output "Existing SS2 UDP block rules still block all other ports. Backup: $backup"
Write-Output 'Restart Host on this port, then have the guest reconnect.'
