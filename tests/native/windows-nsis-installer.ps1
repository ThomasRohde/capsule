[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Installer
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-Condition {
    param(
        [bool]$Condition,
        [string]$Message
    )

    if (-not $Condition) {
        throw $Message
    }
}

function Get-Sha256 {
    param([string]$Path)

    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Invoke-SilentPackage {
    param(
        [string]$Path,
        [string]$Label
    )

    $process = Start-Process -FilePath $Path -ArgumentList '/S' -Wait -PassThru -WindowStyle Hidden
    Assert-Condition ($process.ExitCode -eq 0) "$Label exited $($process.ExitCode)."
}

function Wait-ForPathToDisappear {
    param(
        [string]$Path,
        [string]$Label
    )

    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while ((Test-Path -LiteralPath $Path) -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 100
    }
    Assert-Condition (-not (Test-Path -LiteralPath $Path)) "$Label was not removed."
}

Assert-Condition ($env:OS -eq 'Windows_NT') 'The NSIS installer acceptance gate is Windows-only.'

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path
$tauriConfigPath = Join-Path $repoRoot 'native\desktop\src-tauri\tauri.conf.json'
$expectedDisplayVersion = [string](Get-Content -LiteralPath $tauriConfigPath -Raw | ConvertFrom-Json).version
Assert-Condition (-not [string]::IsNullOrWhiteSpace($expectedDisplayVersion)) 'The authoritative Tauri version is absent.'
$targetRoot = [IO.Path]::GetFullPath((Join-Path $repoRoot 'native\target')).TrimEnd('\') + '\'
$candidate = if ([IO.Path]::IsPathRooted($Installer)) {
    $Installer
} else {
    Join-Path (Get-Location).Path $Installer
}
$installerPath = (Resolve-Path -LiteralPath $candidate).Path
$installerItem = Get-Item -LiteralPath $installerPath
Assert-Condition (-not $installerItem.PSIsContainer) 'The installer must be a regular file.'
Assert-Condition (($installerItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) 'The installer must not be a reparse point.'
Assert-Condition ($installerPath.StartsWith($targetRoot, [StringComparison]::OrdinalIgnoreCase)) 'The installer must be beneath native/target.'
Assert-Condition ([IO.Path]::GetExtension($installerPath) -eq '.exe') 'The installer must be an NSIS executable.'
Assert-Condition ((Get-AuthenticodeSignature -LiteralPath $installerPath).Status -eq 'NotSigned') 'This development gate requires an explicitly unsigned package.'

$installDir = Join-Path $env:LOCALAPPDATA 'SQLite Capsule Host'
$applicationPath = Join-Path $installDir 'sqlite-capsule-desktop.exe'
$uninstallerPath = Join-Path $installDir 'uninstall.exe'
$cachedInstallerPath = Join-Path $installDir 'installer-cache\sqlite-capsule-host-current.exe'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\SQLite Capsule Host'
$productKey = 'HKCU:\Software\sqlite-capsule\SQLite Capsule Host'
$extensionKey = 'Registry::HKEY_CURRENT_USER\Software\Classes\.sqlitecapsule'
$hostClassKey = 'Registry::HKEY_CURRENT_USER\Software\Classes\SQLite Capsule'
$openCommandKey = "$hostClassKey\shell\open\command"
$desktopShortcut = Join-Path ([Environment]::GetFolderPath('Desktop')) 'SQLite Capsule Host.lnk'
$startShortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'SQLite Capsule Host.lnk'
$localApplicationData = Join-Path $env:LOCALAPPDATA 'org.sqlite-capsule.host'
$roamingApplicationData = Join-Path $env:APPDATA 'org.sqlite-capsule.host'
$installerSha256 = Get-Sha256 $installerPath

$acceptanceOwnerName = 'SQLiteCapsule.AcceptanceOwner'
$acceptanceChoiceName = 'SQLiteCapsule.AcceptanceUserChoice'
$acceptanceOwnerKey = "Registry::HKEY_CURRENT_USER\Software\Classes\$acceptanceOwnerName"
$acceptanceChoiceKey = "Registry::HKEY_CURRENT_USER\Software\Classes\$acceptanceChoiceName"
$acceptanceOwnerDescription = 'SQLite Capsule installer acceptance owner'
$acceptanceChoiceDescription = 'SQLite Capsule installer acceptance user choice'
$temporaryAssociationCreated = $false

function Get-ProductState {
    return [ordered]@{
        install_dir = Test-Path -LiteralPath $installDir
        uninstall_key = Test-Path -LiteralPath $uninstallKey
        product_key = Test-Path -LiteralPath $productKey
        extension_key = Test-Path -LiteralPath $extensionKey
        host_class_key = Test-Path -LiteralPath $hostClassKey
        desktop_shortcut = Test-Path -LiteralPath $desktopShortcut
        start_shortcut = Test-Path -LiteralPath $startShortcut
    }
}

function Assert-NoHostProductState {
    param([bool]$AllowExtension = $false)

    $state = Get-ProductState
    foreach ($entry in $state.GetEnumerator()) {
        if ($AllowExtension -and $entry.Key -eq 'extension_key') {
            continue
        }
        Assert-Condition (-not $entry.Value) "Host product state remains: $($entry.Key)."
    }
    Assert-Condition (@(Get-Process -Name 'sqlite-capsule-desktop' -ErrorAction SilentlyContinue).Count -eq 0) 'A host process is still running.'
}

function Get-ShortcutTarget {
    param([string]$Path)

    $shell = New-Object -ComObject WScript.Shell
    return $shell.CreateShortcut($Path).TargetPath
}

function Assert-InstalledState {
    param(
        [int]$ExpectedBackupPresence,
        [string]$ExpectedBackup
    )

    Assert-Condition (Test-Path -LiteralPath $applicationPath -PathType Leaf) 'The installed host executable is missing.'
    Assert-Condition (Test-Path -LiteralPath $uninstallerPath -PathType Leaf) 'The uninstaller is missing.'
    Assert-Condition (Test-Path -LiteralPath $cachedInstallerPath -PathType Leaf) 'The retained installer is missing.'
    Assert-Condition ((Get-Sha256 $cachedInstallerPath) -eq $installerSha256) 'The retained installer differs from the executed package.'
    Assert-Condition (Test-Path -LiteralPath $uninstallKey) 'The HKCU uninstall key is missing.'
    Assert-Condition (Test-Path -LiteralPath $productKey) 'The installer product key is missing.'
    Assert-Condition (Test-Path -LiteralPath $extensionKey) 'The .sqlitecapsule association is missing.'
    Assert-Condition (Test-Path -LiteralPath $hostClassKey) 'The host application class is missing.'

    $sentinel = Get-ItemPropertyValue -LiteralPath $productKey -Name 'AssociationBackupWasPresent'
    Assert-Condition ($sentinel -eq $ExpectedBackupPresence) 'The association-presence sentinel differs.'
    Assert-Condition ((Get-ItemPropertyValue -LiteralPath $extensionKey -Name '(default)') -eq 'SQLite Capsule') 'The installed extension owner differs.'
    Assert-Condition ((Get-ItemPropertyValue -LiteralPath $extensionKey -Name 'SQLite Capsule_backup') -eq $ExpectedBackup) 'The saved association owner differs.'

    $expectedCommand = "`"$applicationPath`" `"%1`""
    Assert-Condition ((Get-ItemPropertyValue -LiteralPath $openCommandKey -Name '(default)') -eq $expectedCommand) 'The open command does not quote both the executable and selected file.'

    $uninstallValues = Get-ItemProperty -LiteralPath $uninstallKey
    Assert-Condition ($uninstallValues.DisplayName -eq 'SQLite Capsule Host') 'The uninstall display name differs.'
    Assert-Condition ($uninstallValues.DisplayVersion -eq $expectedDisplayVersion) 'The uninstall display version differs.'
    Assert-Condition ($uninstallValues.Publisher -eq 'sqlite-capsule') 'The uninstall publisher differs.'
    Assert-Condition ($uninstallValues.NoModify -eq 1) 'The package unexpectedly advertises Modify.'
    Assert-Condition ($uninstallValues.NoRepair -eq 1) 'The package unexpectedly advertises Repair.'

    Assert-Condition (Test-Path -LiteralPath $desktopShortcut -PathType Leaf) 'The silent installer did not create its desktop shortcut.'
    Assert-Condition (Test-Path -LiteralPath $startShortcut -PathType Leaf) 'The installer did not create its Start Menu shortcut.'
    Assert-Condition ((Get-ShortcutTarget $desktopShortcut) -eq $applicationPath) 'The desktop shortcut target differs.'
    Assert-Condition ((Get-ShortcutTarget $startShortcut) -eq $applicationPath) 'The Start Menu shortcut target differs.'
    Assert-Condition (@(Get-Process -Name 'sqlite-capsule-desktop' -ErrorAction SilentlyContinue).Count -eq 0) 'A silent install unexpectedly launched the host.'

    return [ordered]@{
        association_backup_was_present = $sentinel
        association_backup = Get-ItemPropertyValue -LiteralPath $extensionKey -Name 'SQLite Capsule_backup'
        cached_installer_sha256 = Get-Sha256 $cachedInstallerPath
        open_command = Get-ItemPropertyValue -LiteralPath $openCommandKey -Name '(default)'
    }
}

function Remove-TemporaryAcceptanceAssociation {
    if (Test-Path -LiteralPath $extensionKey) {
        $extension = Get-Item -LiteralPath $extensionKey
        $unexpectedProperties = @($extension.Property | Where-Object { $_ -notin @('(default)', 'SQLite Capsule_backup') })
        $subkeys = @(Get-ChildItem -LiteralPath $extensionKey)
        $currentOwner = Get-ItemPropertyValue -LiteralPath $extensionKey -Name '(default)' -ErrorAction SilentlyContinue
        Assert-Condition ($unexpectedProperties.Count -eq 0) 'The temporary extension key gained unexpected values; refusing cleanup.'
        Assert-Condition ($subkeys.Count -eq 0) 'The temporary extension key gained subkeys; refusing cleanup.'
        Assert-Condition ($currentOwner -in @('', 'SQLite Capsule', $acceptanceOwnerName, $acceptanceChoiceName)) 'The temporary extension owner changed unexpectedly; refusing cleanup.'
        Remove-Item -LiteralPath $extensionKey -Force
    }

    foreach ($entry in @(
        @($acceptanceOwnerKey, $acceptanceOwnerDescription),
        @($acceptanceChoiceKey, $acceptanceChoiceDescription)
    )) {
        if (-not (Test-Path -LiteralPath $entry[0])) {
            continue
        }
        $key = Get-Item -LiteralPath $entry[0]
        Assert-Condition (@(Get-ChildItem -LiteralPath $entry[0]).Count -eq 0) 'A temporary class gained subkeys; refusing cleanup.'
        Assert-Condition (@($key.Property | Where-Object { $_ -ne '(default)' }).Count -eq 0) 'A temporary class gained values; refusing cleanup.'
        Assert-Condition ((Get-ItemPropertyValue -LiteralPath $entry[0] -Name '(default)') -eq $entry[1]) 'A temporary class changed unexpectedly; refusing cleanup.'
        Remove-Item -LiteralPath $entry[0] -Force
    }
}

$applicationDataBefore = [ordered]@{
    local = Test-Path -LiteralPath $localApplicationData
    roaming = Test-Path -LiteralPath $roamingApplicationData
}
$cleanCycle = $null
$associationCycle = $null

Assert-NoHostProductState
Assert-Condition (-not (Test-Path -LiteralPath $acceptanceOwnerKey)) 'The temporary owner class already exists.'
Assert-Condition (-not (Test-Path -LiteralPath $acceptanceChoiceKey)) 'The temporary user-choice class already exists.'

try {
    Invoke-SilentPackage $installerPath 'clean install'
    $cleanInstall = Assert-InstalledState 0 ''
    Invoke-SilentPackage $installerPath 'clean same-version reinstall'
    $cleanReinstall = Assert-InstalledState 0 ''
    Invoke-SilentPackage $uninstallerPath 'clean uninstall'
    Wait-ForPathToDisappear $installDir 'The clean-cycle install directory'
    Assert-NoHostProductState
    $cleanCycle = [ordered]@{
        install = $cleanInstall
        reinstall = $cleanReinstall
        clean_after_uninstall = $true
    }

    New-Item -Path $extensionKey -Force | Out-Null
    Set-Item -LiteralPath $extensionKey -Value $acceptanceOwnerName
    New-Item -Path $acceptanceOwnerKey -Force | Out-Null
    Set-Item -LiteralPath $acceptanceOwnerKey -Value $acceptanceOwnerDescription
    $temporaryAssociationCreated = $true

    Invoke-SilentPackage $installerPath 'foreign-association install'
    $foreignInstall = Assert-InstalledState 1 $acceptanceOwnerName
    Invoke-SilentPackage $installerPath 'foreign-association same-version reinstall'
    $foreignReinstall = Assert-InstalledState 1 $acceptanceOwnerName

    New-Item -Path $acceptanceChoiceKey -Force | Out-Null
    Set-Item -LiteralPath $acceptanceChoiceKey -Value $acceptanceChoiceDescription
    Set-Item -LiteralPath $extensionKey -Value $acceptanceChoiceName

    Invoke-SilentPackage $uninstallerPath 'foreign-association uninstall'
    Wait-ForPathToDisappear $installDir 'The foreign-association install directory'
    Assert-NoHostProductState -AllowExtension $true
    Assert-Condition ((Get-ItemPropertyValue -LiteralPath $extensionKey -Name '(default)') -eq $acceptanceChoiceName) 'Uninstall overwrote the post-install user choice.'
    Assert-Condition (-not ((Get-Item -LiteralPath $extensionKey).Property -contains 'SQLite Capsule_backup')) 'Uninstall left the private association backup marker.'
    Assert-Condition ((Get-ItemPropertyValue -LiteralPath $acceptanceOwnerKey -Name '(default)') -eq $acceptanceOwnerDescription) 'The pre-existing class changed.'
    Assert-Condition ((Get-ItemPropertyValue -LiteralPath $acceptanceChoiceKey -Name '(default)') -eq $acceptanceChoiceDescription) 'The user-choice class changed.'
    $associationCycle = [ordered]@{
        install = $foreignInstall
        reinstall = $foreignReinstall
        preserved_post_install_user_choice = $true
        removed_private_backup = $true
    }

    Remove-TemporaryAcceptanceAssociation
    $temporaryAssociationCreated = $false
    Assert-NoHostProductState
} finally {
    if (Test-Path -LiteralPath $uninstallerPath -PathType Leaf) {
        try {
            Invoke-SilentPackage $uninstallerPath 'failure cleanup uninstall'
        } catch {
            Write-Warning $_
        }
    }
    if ($temporaryAssociationCreated) {
        Remove-TemporaryAcceptanceAssociation
    }
}

$applicationDataAfter = [ordered]@{
    local = Test-Path -LiteralPath $localApplicationData
    roaming = Test-Path -LiteralPath $roamingApplicationData
}
Assert-Condition ($applicationDataAfter.local -eq $applicationDataBefore.local) 'The installer cycle changed local application-data existence.'
Assert-Condition ($applicationDataAfter.roaming -eq $applicationDataBefore.roaming) 'The installer cycle changed roaming application-data existence.'

[ordered]@{
    format = 'org.sqlite-capsule.windows-nsis-acceptance/0.2'
    identity = [Security.Principal.WindowsIdentity]::GetCurrent().Name
    installer = [ordered]@{
        path = $installerPath
        bytes = $installerItem.Length
        sha256 = $installerSha256
        signature = 'NotSigned'
    }
    clean_cycle = $cleanCycle
    association_ownership_cycle = $associationCycle
    application_data_before = $applicationDataBefore
    application_data_after = $applicationDataAfter
    final_product_state = Get-ProductState
} | ConvertTo-Json -Depth 8
