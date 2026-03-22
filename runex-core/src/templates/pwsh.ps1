# runex shell integration for PowerShell
function __runex_trim_trailing_spaces {
    param([string]$text)

    return $text.TrimEnd(' ')
}

function __runex_is_command_position {
    param([string]$prefix)

    $prefix = __runex_trim_trailing_spaces $prefix
    if (-not $prefix) {
        return $true
    }

    if ($prefix -match '(\|\||&&|\||;)$') {
        return $true
    }

    $parts = $prefix -split ' '
    $prev = $parts[-1]
    if ($prev -eq 'sudo') {
        $before = __runex_trim_trailing_spaces ($prefix.Substring(0, $prefix.Length - 4))
        if (-not $before) {
            return $true
        }
        return [bool]($before -match '(\|\||&&|\||;)$')
    }

    return $false
}

function __runex_is_known_token {
    param([string]$token)

    switch ($token) {
{PWSH_KNOWN_CASES}
        default { return $false }
    }
}

function __runex_get_expand_candidate {
    param(
        [string]$line,
        [int]$cursor
    )

    if ($cursor -lt $line.Length -and $line[$cursor] -ne ' ') {
        return $null
    }

    $left = $line.Substring(0, $cursor)
    $tokenStart = $left.LastIndexOf(' ') + 1
    $prefix = $line.Substring(0, $tokenStart)
    $token = $left.Substring($tokenStart)

    if (-not $token) {
        return $null
    }

    if (-not ((__runex_is_command_position $prefix) -and (__runex_is_known_token $token))) {
        return $null
    }

    return @{
        Prefix = $prefix
        Token = $token
        TokenStart = $tokenStart
    }
}

function __runex_expand_space {
    param(
        [string]$line,
        [int]$cursor,
        $candidate
    )

    $right = $line.Substring($cursor)
    if ($cursor -lt $line.Length -and $line[$cursor] -ne ' ') {
        return @{
            Action = 'insert'
            Line = $line.Substring(0, $cursor) + ' ' + $right
            Cursor = $cursor + 1
        }
    }

    if (-not $candidate) {
        return @{
            Action = 'insert'
            Line = $line.Substring(0, $cursor) + ' ' + $line.Substring($cursor)
            Cursor = $cursor + 1
        }
    }

    $expanded = & {PWSH_BIN} expand "--token=$($candidate.Token)" 2>$null
    if ($expanded -and $expanded -ne $candidate.Token) {
        $line = $line.Substring(0, $candidate.TokenStart) + $expanded + $right
        $cursor = $candidate.TokenStart + $expanded.Length
        return @{
            Action = 'replace'
            Line = $line.Substring(0, $cursor) + ' ' + $line.Substring($cursor)
            Cursor = $cursor + 1
        }
    }

    return @{
        Action = 'insert'
        Line = $line.Substring(0, $cursor) + ' ' + $line.Substring($cursor)
        Cursor = $cursor + 1
    }
}

if (-not (Get-Command Set-PSReadLineKeyHandler -ErrorAction SilentlyContinue)) {
    Import-Module PSReadLine -ErrorAction SilentlyContinue
}

function __runex_register_expand_handler {
    param(
        [string]$chord,
        [string]$viMode
    )

    $params = @{
        Chord = $chord
        ScriptBlock = {
            param($key, $arg)
            $line = $null
            $cursor = $null
            [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cursor)

            $candidate = __runex_get_expand_candidate $line $cursor
            if (-not $candidate) {
                [Microsoft.PowerShell.PSConsoleReadLine]::Insert(' ')
                return
            }

            $state = __runex_expand_space $line $cursor $candidate
            if ($state.Action -eq 'replace') {
                [Microsoft.PowerShell.PSConsoleReadLine]::Replace(0, $line.Length, $state.Line)
                [Microsoft.PowerShell.PSConsoleReadLine]::SetCursorPosition($state.Cursor)
            } else {
                [Microsoft.PowerShell.PSConsoleReadLine]::Insert(' ')
            }
        }
    }

    if ($viMode) {
        $params.ViMode = $viMode
    }

    Set-PSReadLineKeyHandler @params
}

if (Get-Command Set-PSReadLineKeyHandler -ErrorAction SilentlyContinue) {
{PWSH_REGISTER_LINES}
}
