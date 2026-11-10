param([int]$Hwnd = 0, [int]$MaxDepth = 12, [string]$Filter = "")
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

if ($Hwnd -eq 0) {
    $proc = Get-Process agent-control-app | Select-Object -First 1
    $root = [System.Windows.Automation.AutomationElement]::FromHandle($proc.MainWindowHandle)
} else {
    $root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$Hwnd)
}

$walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
$count = 0

function Walk($el, $depth) {
    if ($null -eq $el -or $depth -gt $MaxDepth -or $script:count -gt 400) { return }
    $script:count++
    $name = $el.Current.Name
    $ct = $el.Current.ControlType.ProgrammaticName -replace 'ControlType\.', ''
    $aid = $el.Current.AutomationId
    $patterns = ($el.GetSupportedPatterns() | ForEach-Object { $_.ProgrammaticName -replace 'PatternIdentifiers\.', '' -replace 'Pattern$', '' }) -join ','
    $line = ("  " * $depth) + "[$ct] name='$name' aid='$aid' patterns=$patterns"
    if ($Filter -eq "" -or $name -like "*$Filter*" -or $aid -like "*$Filter*") {
        Write-Output $line
    }
    $child = $walker.GetFirstChild($el)
    while ($null -ne $child) {
        Walk $child ($depth + 1)
        $child = $walker.GetNextSibling($child)
    }
}

Walk $root 0
Write-Output "TOTAL_NODES=$script:count"