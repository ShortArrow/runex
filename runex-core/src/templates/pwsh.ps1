# runex shell integration for PowerShell
Set-PSReadLineKeyHandler -Chord ' ' -ScriptBlock {
    param($key, $arg)
    $line = $null
    $cursor = $null
    [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cursor)

    $right = $line.Substring($cursor)
    if ($cursor -lt $line.Length -and $line[$cursor] -ne ' ') {
        [Microsoft.PowerShell.PSConsoleReadLine]::Insert(' ')
        return
    }

    $left = $line.Substring(0, $cursor)
    $tokenStart = $left.LastIndexOf(' ') + 1
    $token = $left.Substring($tokenStart)
    if ($token) {
        $expanded = & {BIN} expand --token $token 2>$null
        if ($expanded -and $expanded -ne $token) {
            $newLine = $line.Substring(0, $tokenStart) + $expanded + $right
            [Microsoft.PowerShell.PSConsoleReadLine]::Replace(0, $line.Length, $newLine)
            [Microsoft.PowerShell.PSConsoleReadLine]::SetCursorPosition($tokenStart + $expanded.Length)
        }
    }
    [Microsoft.PowerShell.PSConsoleReadLine]::Insert(' ')
}
