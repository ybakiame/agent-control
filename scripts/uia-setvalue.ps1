param([Parameter(Mandatory=$true)][string]$Name, [Parameter(Mandatory=$true)][string]$Value, [int]$Hwnd = 0)
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

if ($Hwnd -eq 0) {
    $proc = Get-Process agent-control-app | Select-Object -First 1
    $root = [System.Windows.Automation.AutomationElement]::FromHandle($proc.MainWindowHandle)
} else {
    $root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$Hwnd)
}

$condName = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $Name)
$condAid = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, $Name)
$cond = New-Object System.Windows.Automation.OrCondition($condName, $condAid)
$el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
if ($null -eq $el) {
    Write-Output "NOT_FOUND: $Name"
    exit 1
}
$pattern = $el.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
$pattern.SetValue($Value)
Write-Output "SET_VALUE: $Name = $Value"