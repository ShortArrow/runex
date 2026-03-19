# runex shell integration for PowerShell
Set-PSReadLineKeyHandler -Chord ' ' -ScriptBlock {
    param($key, $arg)
    $line = $null
    $cursor = $null
    [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cursor)
    $token = ($line -split ' ')[0]
    if ($token) {
        $expanded = & {BIN} expand --token $token 2>$null
        if ($expanded -and $expanded -ne $token) {
            $newLine = $expanded + $line.Substring($token.Length)
            [Microsoft.PowerShell.PSConsoleReadLine]::Replace(0, $line.Length, $newLine)
            [Microsoft.PowerShell.PSConsoleReadLine]::SetCursorPosition($expanded.Length)
        }
    }
    [Microsoft.PowerShell.PSConsoleReadLine]::Insert(' ')
}
