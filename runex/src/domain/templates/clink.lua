-- runex shell integration for clink

local RUNEX_BIN = {CLINK_BIN}

-- Safe-line gate: reject buffers we shouldn't pass through io.popen.
-- Two classes of content are rejected:
--
--   1. ASCII control characters (NUL, C0). These would either truncate
--      cmd.exe's argv parsing or cause the runex CLI's clap parser to
--      bail.
--
--   2. cmd.exe metacharacters that survive double-quote escaping:
--      `%FOO%` is expanded inside *quoted* arguments by cmd.exe, and
--      `!FOO!` is expanded when SETLOCAL ENABLEDELAYEDEXPANSION is in
--      effect anywhere upstream. A buffer like `%PATH%` or
--      `!cmd! & calc & !x!` would be rewritten by cmd before runex hook
--      ever saw it, including being able to inject extra commands.
--      runex_shell_quote escapes only `"` so it cannot defend against
--      these by itself.
--
-- When the buffer contains either, fall through to the trigger key's
-- normal behaviour (literal-space insertion). Users typing literal
-- `%` or `!` lose the runex expansion on that keypress, which is a
-- minor UX trade for closing a real injection class.
--
-- This stays in lua (rather than as a `runex validate-line` subcommand)
-- because the whole point is to short-circuit *before* spending the cost
-- of spawning a cmd.exe + runex.exe.
local function runex_is_safe_line(line)
    return not line:find("[%z\1-\31%%!]")
end

-- cmd.exe quoting: wrap in double quotes and escape embedded double quotes.
-- POSIX single-quote wrapping would be interpreted literally by cmd.exe and
-- fail (e.g. 'runex' would be treated as a file named "'runex'").
local function runex_shell_quote(s)
    return '"' .. s:gsub('"', '\\"') .. '"'
end

local function runex_call_hook(line, cursor)
    if not runex_is_safe_line(line) then
        return nil
    end
    -- io.popen on Windows ultimately calls cmd.exe with the assembled
    -- string. cmd.exe's quote handling (without /S) is heuristic: when the
    -- string starts with `"` AND ends with `"`, cmd strips the outermost
    -- pair before parsing the rest. So we wrap the entire command in an
    -- extra pair of `"` so the inner quoting around argv0 and --line
    -- survives. argv0 itself is quoted in case the binary path contains
    -- spaces (e.g. `Program Files`). Empirically validated by the
    -- runex/tests/clink_cmd_quoting.rs integration tests.
    local cmd = '"' .. runex_shell_quote(RUNEX_BIN)
        .. ' hook --shell clink --line ' .. runex_shell_quote(line)
        .. ' --cursor ' .. tostring(cursor)
        .. ' 2>&1"'
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
    -- clink's cursor is 1-based (position, not byte offset); runex's Rust
    -- side expects a 0-based byte offset into `line`. Subtract 1.
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
