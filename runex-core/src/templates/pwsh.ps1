# runex shell integration for PowerShell
function __runex_expand_space {
    param(
        [string]$line,
        [int]$cursor
    )

    $right = $line.Substring($cursor)
    if ($cursor -lt $line.Length -and $line[$cursor] -ne ' ') {
        return @{
            Line = $line.Substring(0, $cursor) + ' ' + $right
            Cursor = $cursor + 1
        }
    }

    $left = $line.Substring(0, $cursor)
    $tokenStart = $left.LastIndexOf(' ') + 1
    $token = $left.Substring($tokenStart)
    if ($token) {
        $expanded = & {BIN} expand --token $token 2>$null
        if ($expanded -and $expanded -ne $token) {
            $line = $line.Substring(0, $tokenStart) + $expanded + $right
            $cursor = $tokenStart + $expanded.Length
        }
    }

    return @{
        Line = $line.Substring(0, $cursor) + ' ' + $line.Substring($cursor)
        Cursor = $cursor + 1
    }
}

if (-not (Get-Command Set-PSReadLineKeyHandler -ErrorAction SilentlyContinue)) {
    Import-Module PSReadLine -ErrorAction SilentlyContinue
}

function __runex_register_space_handler {
    param(
        [string]$viMode
    )

    $params = @{
        Chord = ' '
        ScriptBlock = {
            param($key, $arg)
            $line = $null
            $cursor = $null
            [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cursor)

            $state = __runex_expand_space $line $cursor
            [Microsoft.PowerShell.PSConsoleReadLine]::Replace(0, $line.Length, $state.Line)
            [Microsoft.PowerShell.PSConsoleReadLine]::SetCursorPosition($state.Cursor)
        }
    }

    if ($viMode) {
        $params.ViMode = $viMode
    }

    Set-PSReadLineKeyHandler @params
}

if (Get-Command Set-PSReadLineKeyHandler -ErrorAction SilentlyContinue) {
    __runex_register_space_handler
    if ((Get-PSReadLineOption).EditMode -eq 'Vi') {
        __runex_register_space_handler Insert
    }
}
