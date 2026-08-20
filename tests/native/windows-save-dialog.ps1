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
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class SQLiteCapsuleDialogFocus
{
    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr window);

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, IntPtr processId);

    [DllImport("kernel32.dll")]
    private static extern uint GetCurrentThreadId();

    [DllImport("user32.dll")]
    private static extern bool AttachThreadInput(uint idAttach, uint idAttachTo, bool attach);

    [DllImport("user32.dll")]
    private static extern bool BringWindowToTop(IntPtr window);

    [DllImport("user32.dll")]
    private static extern bool ShowWindow(IntPtr window, int command);

    [DllImport("user32.dll")]
    private static extern IntPtr SetActiveWindow(IntPtr window);

    [DllImport("user32.dll")]
    private static extern IntPtr SetFocus(IntPtr window);

    private delegate bool EnumWindowsProc(IntPtr window, IntPtr parameter);

    [DllImport("user32.dll")]
    private static extern bool EnumChildWindows(IntPtr parent, EnumWindowsProc callback, IntPtr parameter);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetClassNameW(IntPtr window, StringBuilder className, int maximum);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern bool SetWindowTextW(IntPtr window, string value);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowTextW(IntPtr window, StringBuilder value, int maximum);

    public static bool TrySetExactFileName(IntPtr edit, string value)
    {
        if (edit == IntPtr.Zero || !SetWindowTextW(edit, value)) return false;
        var observed = new StringBuilder(Math.Max(value.Length + 1, 260));
        GetWindowTextW(edit, observed, observed.Capacity);
        return string.Equals(observed.ToString(), value, StringComparison.Ordinal);
    }

    public static bool TryCommitExact(IntPtr button)
    {
        if (button == IntPtr.Zero) return false;
        SendMessageW(button, 0x00F5, IntPtr.Zero, IntPtr.Zero);
        return true;
    }

    [DllImport("user32.dll")]
    private static extern IntPtr GetDlgItem(IntPtr dialog, int controlId);

    [DllImport("user32.dll")]
    private static extern IntPtr SendMessageW(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

    public static bool ForceForeground(IntPtr window)
    {
        uint currentThread = GetCurrentThreadId();
        uint targetThread = GetWindowThreadProcessId(window, IntPtr.Zero);
        IntPtr foreground = GetForegroundWindow();
        uint foregroundThread = foreground == IntPtr.Zero
            ? 0
            : GetWindowThreadProcessId(foreground, IntPtr.Zero);
        bool attachedForeground = foregroundThread != 0 && foregroundThread != currentThread
            && AttachThreadInput(currentThread, foregroundThread, true);
        bool attachedTarget = targetThread != 0 && targetThread != currentThread
            && targetThread != foregroundThread
            && AttachThreadInput(currentThread, targetThread, true);
        try
        {
            ShowWindow(window, 9);
            BringWindowToTop(window);
            SetActiveWindow(window);
            SetFocus(window);
            return SetForegroundWindow(window) || GetForegroundWindow() == window;
        }
        finally
        {
            if (attachedTarget) AttachThreadInput(currentThread, targetThread, false);
            if (attachedForeground) AttachThreadInput(currentThread, foregroundThread, false);
        }
    }

    private static void CollectEdits(IntPtr parent, List<IntPtr> candidates)
    {
        if (parent == IntPtr.Zero) return;
        EnumChildWindows(parent, delegate(IntPtr child, IntPtr parameter)
        {
            var name = new StringBuilder(64);
            if (GetClassNameW(child, name, name.Capacity) > 0
                && string.Equals(name.ToString(), "Edit", StringComparison.OrdinalIgnoreCase))
            {
                candidates.Add(child);
            }
            return true;
        }, IntPtr.Zero);
    }

    public static bool TrySetFileName(IntPtr dialog, string value)
    {
        var candidates = new List<IntPtr>();
        // Common Item Dialog: cmb13 (0x047c) owns the file-name edit. Give it
        // priority over other Edit controls such as the search box.
        CollectEdits(GetDlgItem(dialog, 0x047c), candidates);
        IntPtr classicEdit = GetDlgItem(dialog, 0x0480);
        if (classicEdit != IntPtr.Zero) candidates.Add(classicEdit);
        CollectEdits(dialog, candidates);
        bool updated = false;
        for (int index = 0; index < candidates.Count; index++)
        {
            updated = SetWindowTextW(candidates[index], value) || updated;
        }
        return updated;
    }

    public static bool TryCommit(IntPtr dialog)
    {
        IntPtr button = GetDlgItem(dialog, 1);
        if (button == IntPtr.Zero) return false;
        SendMessageW(button, 0x00F5, IntPtr.Zero, IntPtr.Zero);
        return true;
    }
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
    $legacyPatternObject = $null
    $legacyCandidate = $fileName
    $legacyPattern = [System.Windows.Automation.AutomationPattern]::LookupById(10018)
    if ($null -ne $legacyPattern -and -not $legacyCandidate.TryGetCurrentPattern(
        $legacyPattern,
        [ref] $legacyPatternObject
    )) {
        for ($index = 0; $index -lt $descendants.Count; $index += 1) {
            $candidate = $descendants.Item($index)
            $candidatePattern = $null
            if ($null -ne $legacyPattern -and $candidate.TryGetCurrentPattern(
                $legacyPattern,
                [ref] $candidatePattern
            )) {
                $legacyCandidate = $candidate
                $legacyPatternObject = $candidatePattern
                break
            }
        }
    }
    if ($null -ne $legacyPatternObject) {
        $fileNameInput = $legacyCandidate
        $inputPatternKind = 'LegacyIAccessiblePattern'
        $inputPatternObject = $legacyPatternObject
    }
    else {
        $fileNameInput = $fileName
        $nativeFileNameHandle = [IntPtr] $fileName.Current.NativeWindowHandle
        $nativeSaveButtonHandle = [IntPtr] $saveButton.Current.NativeWindowHandle
        if ([string]::Equals(
            $fileName.Current.Name,
            $leaf,
            [System.StringComparison]::Ordinal
        ) -and $nativeSaveButtonHandle -ne [IntPtr]::Zero) {
            $inputPatternKind = 'HostSuggestedName'
            $inputPatternObject = $nativeSaveButtonHandle
        }
        elseif ($nativeFileNameHandle -ne [IntPtr]::Zero -and
            $nativeSaveButtonHandle -ne [IntPtr]::Zero) {
            $inputPatternKind = 'NativeDialogEdit'
            $inputPatternObject = $nativeFileNameHandle
        }
        else {
            $inputPatternKind = 'WindowsSaveDialogKeyboard'
            $inputPatternObject = [IntPtr] $dialog.Current.NativeWindowHandle
        }
    }
}
if ($null -eq $fileNameInput -or $null -eq $inputPatternObject) {
    throw 'The process-owned file-name host has no writable UI Automation pattern.'
}
$invokePatternObject = $null
$saveButtonSupportsInvoke = $saveButton.TryGetCurrentPattern(
    [System.Windows.Automation.InvokePattern]::Pattern,
    [ref] $invokePatternObject
)
$foregroundRequested = $null

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
if ($inputPatternKind -eq 'ValuePattern' -or $inputPatternKind -eq 'LegacyIAccessiblePattern' -or $inputPatternKind -eq 'NativeDialogEdit' -or $inputPatternKind -eq 'HostSuggestedName') {
    if ($inputPatternKind -notin @('NativeDialogEdit', 'HostSuggestedName') -and
        (-not $saveButtonSupportsInvoke -or $null -eq $invokePatternObject)) {
        throw 'The process-owned Save button does not support InvokePattern.'
    }
    if ($inputPatternKind -eq 'HostSuggestedName') {
        # The host owns the bounded default leaf. Leaving it untouched exercises
        # the real production picker without relying on locale-specific keys.
    }
    elseif ($inputPatternKind -eq 'ValuePattern') {
        ([System.Windows.Automation.ValuePattern] $inputPatternObject).SetValue($canonicalDestination)
    }
    elseif ($inputPatternKind -eq 'LegacyIAccessiblePattern') {
        $inputPatternObject.SetValue($canonicalDestination)
    }
    elseif (-not [SQLiteCapsuleDialogFocus]::TrySetExactFileName(
        [IntPtr] $inputPatternObject,
        $canonicalDestination
    )) {
        throw 'The process-owned save dialog exposes no writable native file-name edit.'
    }
    if ($saveButtonSupportsInvoke -and $null -ne $invokePatternObject) {
        ([System.Windows.Automation.InvokePattern] $invokePatternObject).Invoke()
        $saveCommitMethod = 'InvokePattern'
    }
    elseif ([SQLiteCapsuleDialogFocus]::TryCommitExact(
        [IntPtr] $saveButton.Current.NativeWindowHandle
    )) {
        $saveCommitMethod = 'NativeDialogButton'
    }
    else {
        throw 'The process-owned Save button has no invokable native control.'
    }
}
else {
    $dialogHandle = [IntPtr] $inputPatternObject
    if ($dialogHandle -eq [IntPtr]::Zero) {
        throw 'The process-owned save dialog has no native window handle.'
    }
    $foregroundRequested = [SQLiteCapsuleDialogFocus]::ForceForeground($dialogHandle)
    Start-Sleep -Milliseconds 100
    # Windows may reject the observable foreground transition for a process-
    # owned common dialog while still accepting keyboard input after the UIA
    # SetFocus calls above. Continue and prove success from the create-new output
    # plus the dialog-owned report instead of treating foreground telemetry as
    # authority.
    if ($canonicalDestination.IndexOfAny([char[]] '+^%~()[]{}') -ge 0) {
        throw 'The isolated acceptance path contains unsupported SendKeys metacharacters.'
    }
    $keyboard = New-Object -ComObject WScript.Shell
    if (-not $keyboard.AppActivate([int] $HostProcessId)) {
        throw 'The process-owned save dialog could not be activated for keyboard input.'
    }
    Start-Sleep -Milliseconds 100
    try {
        $dialog.SetFocus()
        $fileNameInput.SetFocus()
    }
    catch {
        # The exact modal dialog is already process-owned and active; the
        # subsequent create-new output remains the acceptance authority.
    }
    $keyboard.SendKeys('^a')
    $keyboard.SendKeys($canonicalDestination)
    Start-Sleep -Milliseconds 200
    $keyboard.SendKeys('{ENTER}')
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
    foreground_requested = $foregroundRequested
    destination = $canonicalDestination
} | ConvertTo-Json -Compress
