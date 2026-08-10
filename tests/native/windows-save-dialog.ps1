[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateRange(1, 2147483647)]
    [uint32] $HostProcessId,

    [Parameter(Mandatory = $true)]
    [string] $Destination,

    [Parameter(Mandatory = $true)]
    [string] $StateRoot,

    [ValidateRange(1, 60)]
    [int] $TimeoutSeconds = 20
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($Destination.IndexOf([char]0) -ge 0) {
    throw 'The destination contains a NUL character.'
}
if (-not [System.IO.Path]::IsPathRooted($Destination)) {
    throw 'The destination must be an absolute path.'
}
$normalizedDestination = [System.IO.Path]::GetFullPath($Destination)
if (-not [string]::Equals(
    $normalizedDestination,
    $Destination,
    [System.StringComparison]::OrdinalIgnoreCase
)) {
    throw 'The destination must be an absolute, normalized path.'
}
if ([System.IO.Path]::GetExtension($Destination) -ne '.sqlitecapsule') {
    throw 'The destination must use the .sqlitecapsule extension.'
}
if (Test-Path -LiteralPath $Destination) {
    throw 'The destination already exists.'
}

$parent = [System.IO.Path]::GetDirectoryName($normalizedDestination)
$leaf = [System.IO.Path]::GetFileName($normalizedDestination)
if ([string]::IsNullOrWhiteSpace($parent) -or [string]::IsNullOrWhiteSpace($leaf)) {
    throw 'The destination must have an existing parent and a file name.'
}
$canonicalParent = (Resolve-Path -LiteralPath $parent -ErrorAction Stop).ProviderPath
$canonicalDestination = [System.IO.Path]::Combine($canonicalParent, $leaf)
$canonicalStateRoot = (Resolve-Path -LiteralPath $StateRoot -ErrorAction Stop).ProviderPath
$statePrefix = $canonicalStateRoot.TrimEnd(
    [System.IO.Path]::DirectorySeparatorChar,
    [System.IO.Path]::AltDirectorySeparatorChar
) + [System.IO.Path]::DirectorySeparatorChar
if (-not [string]::Equals(
    $canonicalParent,
    $canonicalStateRoot,
    [System.StringComparison]::OrdinalIgnoreCase
) -and -not $canonicalParent.StartsWith(
    $statePrefix,
    [System.StringComparison]::OrdinalIgnoreCase
)) {
    throw 'The destination parent must remain beneath the isolated state root.'
}

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class SQLiteCapsuleDialogFocus
{
    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr window);

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();
}
'@

$processCondition = [System.Windows.Automation.PropertyCondition]::new(
    [System.Windows.Automation.AutomationElement]::ProcessIdProperty,
    [int] $HostProcessId
)
$windowCondition = [System.Windows.Automation.PropertyCondition]::new(
    [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
    [System.Windows.Automation.ControlType]::Window
)
$topLevelCondition = [System.Windows.Automation.AndCondition]::new(
    $processCondition,
    $windowCondition
)
$fileNameCondition = [System.Windows.Automation.PropertyCondition]::new(
    [System.Windows.Automation.AutomationElement]::AutomationIdProperty,
    '1001'
)
$saveButtonCondition = [System.Windows.Automation.PropertyCondition]::new(
    [System.Windows.Automation.AutomationElement]::AutomationIdProperty,
    '1'
)

$deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
$dialog = $null
$fileName = $null
$saveButton = $null
do {
    $windows = [System.Windows.Automation.AutomationElement]::RootElement.FindAll(
        [System.Windows.Automation.TreeScope]::Children,
        $topLevelCondition
    )
    foreach ($window in $windows) {
        $candidateFileName = $window.FindFirst(
            [System.Windows.Automation.TreeScope]::Descendants,
            $fileNameCondition
        )
        $candidateSaveButton = $window.FindFirst(
            [System.Windows.Automation.TreeScope]::Descendants,
            $saveButtonCondition
        )
        if ($null -ne $candidateFileName -and $null -ne $candidateSaveButton) {
            $dialog = $window
            $fileName = $candidateFileName
            $saveButton = $candidateSaveButton
            break
        }
    }
    if ($null -eq $dialog) {
        Start-Sleep -Milliseconds 100
    }
} while ($null -eq $dialog -and [DateTime]::UtcNow -lt $deadline)

if ($null -eq $dialog) {
    $diagnostics = @()
    $allWindows = [System.Windows.Automation.AutomationElement]::RootElement.FindAll(
        [System.Windows.Automation.TreeScope]::Children,
        $windowCondition
    )
    foreach ($window in $allWindows) {
        try {
            if ($window.Current.ProcessId -ne [int] $HostProcessId -and $window.Current.ClassName -ne '#32770') {
                continue
            }
            $controlCondition = [System.Windows.Automation.OrCondition]::new(
                [System.Windows.Automation.PropertyCondition]::new(
                    [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
                    [System.Windows.Automation.ControlType]::Edit
                ),
                [System.Windows.Automation.PropertyCondition]::new(
                    [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
                    [System.Windows.Automation.ControlType]::Button
                )
            )
            $matchingControls = $window.FindAll(
                [System.Windows.Automation.TreeScope]::Descendants,
                $controlCondition
            )
            $controls = @()
            for ($index = 0; $index -lt [Math]::Min($matchingControls.Count, 32); $index += 1) {
                $control = $matchingControls.Item($index)
                $controls += [pscustomobject]@{
                    type = $control.Current.ControlType.ProgrammaticName
                    name = $control.Current.Name
                    automation_id = $control.Current.AutomationId
                    class = $control.Current.ClassName
                    enabled = $control.Current.IsEnabled
                }
            }
            $diagnostics += [pscustomobject]@{
                name = $window.Current.Name
                class = $window.Current.ClassName
                process_id = $window.Current.ProcessId
                controls = $controls
            }
        }
        catch {
            continue
        }
    }
    $diagnosticJson = $diagnostics | ConvertTo-Json -Compress -Depth 5
    throw "No process-owned Windows save dialog appeared for PID $HostProcessId. Candidates: $diagnosticJson"
}

$fileNameInput = $fileName
$valuePatternObject = $null
$inputPatternKind = 'ValuePattern'
if (-not $fileNameInput.TryGetCurrentPattern(
    [System.Windows.Automation.ValuePattern]::Pattern,
    [ref] $valuePatternObject
)) {
    $fileNameInput = $null
    $descendants = $fileName.FindAll(
        [System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.Condition]::TrueCondition
    )
    for ($index = 0; $index -lt $descendants.Count; $index += 1) {
        $candidate = $descendants.Item($index)
        $candidatePattern = $null
        if ($candidate.TryGetCurrentPattern(
            [System.Windows.Automation.ValuePattern]::Pattern,
            [ref] $candidatePattern
        )) {
            $fileNameInput = $candidate
            $valuePatternObject = $candidatePattern
            break
        }
    }
}
$inputPatternObject = $valuePatternObject
if ($null -eq $fileNameInput -or $null -eq $inputPatternObject) {
    $fileNameInput = $fileName
    $inputPatternKind = 'WindowsSaveDialogKeyboard'
    $inputPatternObject = [IntPtr] $dialog.Current.NativeWindowHandle
}
if ($null -eq $fileNameInput -or $null -eq $inputPatternObject) {
    throw 'The process-owned file-name host has no writable UI Automation pattern.'
}
$invokePatternObject = $null
$saveButtonSupportsInvoke = $saveButton.TryGetCurrentPattern(
    [System.Windows.Automation.InvokePattern]::Pattern,
    [ref] $invokePatternObject
)

try {
    $dialog.SetFocus()
}
catch {
    # The native dialog remains addressable through its exact UIA elements.
}
try {
    $fileNameInput.SetFocus()
}
catch {
    # Some Windows 11 file-dialog proxies expose a native HWND but reject the
    # optional UIA focus call. SetWindowTextW below targets that HWND directly.
}
if ($inputPatternKind -eq 'ValuePattern') {
    if (-not $saveButtonSupportsInvoke -or $null -eq $invokePatternObject) {
        throw 'The process-owned Save button does not support InvokePattern.'
    }
    ([System.Windows.Automation.ValuePattern] $inputPatternObject).SetValue($canonicalDestination)
    ([System.Windows.Automation.InvokePattern] $invokePatternObject).Invoke()
    $saveCommitMethod = 'InvokePattern'
}
else {
    $dialogHandle = [IntPtr] $inputPatternObject
    if ($dialogHandle -eq [IntPtr]::Zero) {
        throw 'The process-owned save dialog has no native window handle.'
    }
    [SQLiteCapsuleDialogFocus]::SetForegroundWindow($dialogHandle) | Out-Null
    Start-Sleep -Milliseconds 100
    if ([SQLiteCapsuleDialogFocus]::GetForegroundWindow() -ne $dialogHandle) {
        throw 'The process-owned save dialog could not receive the foreground.'
    }
    if ($canonicalDestination.IndexOfAny([char[]] '+^%~()[]{}') -ge 0) {
        throw 'The isolated acceptance path contains unsupported SendKeys metacharacters.'
    }
    [System.Windows.Forms.SendKeys]::SendWait('%n')
    [System.Windows.Forms.SendKeys]::SendWait('^a')
    [System.Windows.Forms.SendKeys]::SendWait($canonicalDestination)
    Start-Sleep -Milliseconds 200
    [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
    $saveCommitMethod = 'KeyboardEnter'
}

[pscustomobject]@{
    ok = $true
    host_process_id = $HostProcessId
    dialog_name = $dialog.Current.Name
    dialog_class = $dialog.Current.ClassName
    file_name_host_automation_id = $fileName.Current.AutomationId
    file_name_input_automation_id = $fileNameInput.Current.AutomationId
    file_name_input_control_type = $fileNameInput.Current.ControlType.ProgrammaticName
    file_name_input_class = $fileNameInput.Current.ClassName
    file_name_input_pattern = $inputPatternKind
    save_button_automation_id = $saveButton.Current.AutomationId
    save_button_supports_invoke = $saveButtonSupportsInvoke
    save_commit_method = $saveCommitMethod
    destination = $canonicalDestination
} | ConvertTo-Json -Compress
