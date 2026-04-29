# runex shell integration for PowerShell

# Paste detection via PSReadLine internal key queue.
# When a clipboard paste is processed, PSReadLine enqueues all pasted
# characters at once — so when our Spacebar handler fires on the first space
# inside a paste, there are still unread keys waiting in _queuedKeys. Manual
# typing goes through the same queue but one key at a time, so the count is 0.
#
# We access _queuedKeys via reflection. If the probe fails (PSReadLine API
# change, etc.) we fall back to 0 and expansion runs as normal — we never
# crash the handler.
#
# This logic stays in pwsh (rather than being lifted into Rust) on purpose:
# the only way to read _queuedKeys is to reach into the live PSReadLine
# singleton inside the running pwsh process. A Rust subprocess can't see
# that state. The state we care about gets forwarded to `runex hook` via
# the `--paste-pending` flag once we've decided here.
$Script:__runex_psrl_singleton_field = $null
$Script:__runex_psrl_queued_keys_field = $null
try {
    $__runex_psrl_type = [Microsoft.PowerShell.PSConsoleReadLine]
    $__runex_bf = [System.Reflection.BindingFlags]'NonPublic,Static,Instance'
    $Script:__runex_psrl_singleton_field   = $__runex_psrl_type.GetField('_singleton',   $__runex_bf)
    $Script:__runex_psrl_queued_keys_field = $__runex_psrl_type.GetField('_queuedKeys', $__runex_bf)
} catch {
    # PSReadLine internals changed — fields stay null and we fall back gracefully.
}

function __runex_queued_key_count {
    if ($null -eq $Script:__runex_psrl_singleton_field -or
        $null -eq $Script:__runex_psrl_queued_keys_field) {
        return 0
    }
    try {
        $inst = $Script:__runex_psrl_singleton_field.GetValue($null)
        if ($null -eq $inst) { return 0 }
        $q = $Script:__runex_psrl_queued_keys_field.GetValue($inst)
        if ($null -eq $q) { return 0 }
        [System.Threading.Monitor]::Enter($q)
        try { return $q.Count }
        finally { [System.Threading.Monitor]::Exit($q) }
    } catch {
        return 0
    }
}

if (-not (Get-Command Set-PSReadLineKeyHandler -ErrorAction SilentlyContinue)) {
    Import-Module PSReadLine -ErrorAction SilentlyContinue
}

function __runex_register_expand_handler {
    param([string]$chord, [string]$viMode)

    $params = @{
        Chord = $chord
        ScriptBlock = {
            param($key, $arg)

            $line = $null
            $cursor = $null
            [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cursor)

            $pastePending = (__runex_queued_key_count) -gt 0

            # The hook emits two lines: __RUNEX_LINE and __RUNEX_CURSOR.
            # pwsh_quote_string produces a double-quoted literal, so the
            # values are safe to Invoke-Expression. On any failure we
            # fall back to plain space insertion.
            $out = $null
            try {
                $hookArgs = @('hook', '--shell', 'pwsh', '--line', $line, '--cursor', "$cursor")
                if ($pastePending) { $hookArgs += '--paste-pending' }
                $out = & {PWSH_BIN} @hookArgs 2>$null
            } catch {
                $out = $null
            }

            if ($out) {
                $__RUNEX_LINE = $null
                $__RUNEX_CURSOR = $null
                try {
                    Invoke-Expression ($out -join "`n")
                } catch {
                    $__RUNEX_LINE = $null
                }
                if ($null -ne $__RUNEX_LINE -and $null -ne $__RUNEX_CURSOR) {
                    [Microsoft.PowerShell.PSConsoleReadLine]::Replace(0, $line.Length, $__RUNEX_LINE)
                    [Microsoft.PowerShell.PSConsoleReadLine]::SetCursorPosition([int]$__RUNEX_CURSOR)
                    return
                }
            }

            [Microsoft.PowerShell.PSConsoleReadLine]::Insert(' ')
        }
    }

    if ($viMode) { $params.ViMode = $viMode }
    Set-PSReadLineKeyHandler @params
}

if (Get-Command Set-PSReadLineKeyHandler -ErrorAction SilentlyContinue) {
{PWSH_REGISTER_LINES}
{PWSH_SELF_INSERT_LINES}
}
