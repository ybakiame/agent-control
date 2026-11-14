param([Parameter(Mandatory=$true)][string]$Name, [int]$Hwnd = 0, [switch]$Last)
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

if ($Hwnd -eq 0) {
    $proc = Get-Process agent-control-app | Select-Object -First 1
    $root = [System.Windows.Automation.AutomationElement]::FromHandle($proc.MainWindowHandle)
} else {
    $root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$Hwnd)
}

$cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $Name)
if ($Last) {
    $all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond)
    if ($all.Count -eq 0) {
        Write-Output "NOT_FOUND: $Name"
        exit 1
    }
    $el = $all[$all.Count - 1]
} else {
    $el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
    if ($null -eq $el) {
        Write-Output "NOT_FOUND: $Name"
        exit 1
    }
}
$pattern = $el.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
$pattern.Invoke()
Write-Output "INVOKED: $Name"