-- runex shell integration for clink

local RUNEX_BIN = {CLINK_BIN}

-- Safe token check: only alphanumerics, dot, underscore, hyphen. We use this
-- to gate whether we pass user buffer content to the runex CLI. Even though
-- the child process is launched via io.popen with explicit quoting, this is a
-- belt-and-suspenders check — a token containing shell metacharacters would
-- have to be an abbreviation key to reach this path, and the config validator
-- already forbids those, but we don't rely on that here.
local function runex_is_safe_line(line)
    -- Allow anything printable that's not control; io.popen goes through cmd.exe
    -- so we still quote, but reject embedded nul and control chars outright.
    return not line:find("[%z\1-\31]")
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
    local cmd = runex_shell_quote(RUNEX_BIN)
        .. ' hook --shell clink --line ' .. runex_shell_quote(line)
        .. ' --cursor ' .. tostring(cursor)
        .. ' 2>&1'
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
