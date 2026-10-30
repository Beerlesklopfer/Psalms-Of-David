# SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
# SPDX-License-Identifier: AGPL-3.0-or-later
# Sets David up as a Windows service that starts with the computer. Run it from an elevated PowerShell (as administrator):
#   .\install-service.ps1 -MediaFolder D:\Hoerbuecher, D:\Musik
# The folders of the audiobooks and the music are chosen afterwards on the pages of the administration (per user);
# -MediaFolder only gives the service the right to read them. The data folder (database, certificate) is made for the service.
param(
    [string[]]$MediaFolder = @(),
    [string]$DataDir = (Join-Path $env:ProgramData 'David')
)
$ErrorActionPreference = 'Stop'
$exe = Join-Path $PSScriptRoot 'david.exe'
if (-not (Test-Path $exe)) { throw "david.exe is not next to this script ($PSScriptRoot)." }

New-Item -ItemType Directory -Force $DataDir | Out-Null
# The service runs as LocalService: it writes the data folder and reads the media folders.
icacls $DataDir /grant 'NT AUTHORITY\LocalService:(OI)(CI)M' | Out-Null
foreach ($folder in $MediaFolder) {
    if (-not (Test-Path $folder -PathType Container)) { throw "The folder $folder does not exist." }
    icacls $folder /grant 'NT AUTHORITY\LocalService:(OI)(CI)RX' | Out-Null
}

& $exe install --data-dir $DataDir
if ($LASTEXITCODE -ne 0) { throw 'The service could not be registered.' }
sc.exe start DavidServer | Out-Null
Write-Host "David is running as the service DavidServer. Data: $DataDir. Remove it with: $exe uninstall"
