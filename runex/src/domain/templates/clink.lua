-- runex shell integration for clink

local RUNEX_BIN = {CLINK_BIN}

-- The only route from clink's lua to runex is io.popen, i.e. a cmd.exe
-- command line, and cmd.exe cannot carry arbitrary buffer text inside an
-- argument: `"` toggles its quote state and has no escape (a `\"` in the
-- buffer closed the argument early and turned `2>&1` into a runex
-- argument, issues #22 and #23), `%VAR%` expands even inside quotes,
-- `!VAR!` expands under delayed expansion, and C0 control characters
-- truncate parsing. Instead of escaping each case, the buffer travels as
-- hex of its UTF-8 bytes: the wire alphabet is [0-9A-F], which cmd.exe
-- passes through untouched, and `runex hook --line-hex` decodes it.
-- Rationale and alternatives: docs/decisions/0003-clink-hex-line-transport.md
local function runex_hex(s)
    return (s:gsub('.', function(c) return string.format('%02X', c:byte()) end))
end

-- cmd.exe's documented command-line limit is 8191 characters, but that
-- counts the `cmd.exe /c ` prefix io.popen adds: measured through
-- `cmd /c`, the longest string cmd.exe still runs is 8158 characters.
-- Hex doubles the buffer, so the assembled command is measured before
-- spawning and the trigger key falls back to a literal space when it
-- would not fit. 8000 leaves headroom for a long %COMSPEC% path.
local CMD_LINE_MAX = 8000

-- cmd.exe quoting for the binary path: wrap in double quotes. POSIX
-- single-quote wrapping would be interpreted literally by cmd.exe and
-- fail (e.g. 'runex' would be treated as a file named "'runex'"). Only
-- RUNEX_BIN goes through this; the buffer is hex-encoded instead.
local function runex_shell_quote(s)
    return '"' .. s:gsub('"', '\\"') .. '"'
end

local function runex_call_hook(line, cursor)
    -- Nothing to expand in an empty buffer, and `--line-hex` with an
    -- empty value would be collapsed by cmd.exe into a missing value.
    -- Skip the spawn; the caller inserts the literal space.
    if line == "" then return nil end
    -- io.popen on Windows ultimately calls cmd.exe with the assembled
    -- string. cmd.exe's quote handling (without /S) is heuristic: when the
    -- string starts with `"` AND ends with `"`, cmd strips the outermost
    -- pair before parsing the rest. So we wrap the entire command in an
    -- extra pair of `"` so the quoting around argv0 survives. argv0 is
    -- quoted in case the binary path contains spaces (e.g. `Program
    -- Files`). The layout is mirrored by
    -- `hook_clink_cmd_exe_roundtrip_keeps_double_quote_in_buffer` in
    -- runex/tests/cli_integration.rs, which runs it through a real cmd.exe.
    local cmd = '"' .. runex_shell_quote(RUNEX_BIN)
        .. ' hook --shell clink --line-hex ' .. runex_hex(line)
        .. ' --cursor ' .. tostring(cursor)
        .. ' 2>&1"'
    if #cmd > CMD_LINE_MAX then return nil end
    local handle = io.popen(cmd)
    if not handle then return nil end
    local out = handle:read("*a")
    handle:close()
    if not out or out == "" then return nil end
    -- The hook emits a `return { line = "...", cursor = N }` Lua literal.
    local chunk, err = load(out, "=runex_hook", "t", {})
    if not chunk then return nil end
    local ok, result = pcall(chunk)
    if not ok or type(result) ~= "table" then return nil end
    if type(result.line) ~= "string" or type(result.cursor) ~= "number" then
        return nil
    end
    return result
end

function runex_expand(rl_buffer, line_state)
    local line = rl_buffer:getbuffer()
    local cursor = rl_buffer:getcursor()
    -- clink's cursor is 1-based (position, not char offset); runex's Rust
    -- side expects a 0-based char offset into `line` (app/hook.rs counts
    -- chars for clink). Subtract 1.
    local result = runex_call_hook(line, cursor - 1)
    if result and result.line ~= line then
        rl_buffer:beginundogroup()
        rl_buffer:remove(1, rl_buffer:getlength() + 1)
        rl_buffer:insert(result.line)
        rl_buffer:setcursor(result.cursor + 1)
        rl_buffer:endundogroup()
    else
        rl_buffer:insert(" ")
    end
end

rl.describemacro([["luafunc:runex_expand"]], "Expand a runex abbreviation and insert a space")
{CLINK_BINDING}
