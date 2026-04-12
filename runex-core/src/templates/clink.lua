-- runex shell integration for clink
local RUNEX_BIN = {CLINK_BIN}
-- Pre-compute command existence cache
local precache_handle = io.popen(RUNEX_BIN .. " precache --shell clink 2>nul")
if precache_handle then
    local precache_line = precache_handle:read("*l")
    precache_handle:close()
    if precache_line then os.execute(precache_line) end
end
local RUNEX_KNOWN = {
{CLINK_KNOWN_CASES}
}

local function runex_trim_trailing_spaces(text)
    return text:gsub("%s+$", "")
end

local function runex_is_command_position(prefix)
    prefix = runex_trim_trailing_spaces(prefix)
    if prefix == "" then
        return true
    end

    if prefix:match("(%|%||&&|;)$") then
        return true
    end

    local prev = prefix:match("([^ ]+)$")
    if prev == "sudo" then
        local before = runex_trim_trailing_spaces(prefix:sub(1, #prefix - 4))
        if before == "" then
            return true
        end
        return before:match("(%|%||&&|;)$") ~= nil
    end

    return false
end

local function runex_is_safe_token(token)
    -- Whitelist: only alphanumeric, dot, underscore, hyphen.
    -- This is a prerequisite for runex_shell_quote safety: tokens that pass this
    -- check contain no shell metacharacters, so shell quoting is a defence-in-depth
    -- measure rather than the sole guard. Do not relax this whitelist without
    -- also auditing the io.popen command construction below.
    return token:match("^[%w%._%-]+$") ~= nil
end

local function runex_shell_quote(s)
    -- Wrap s in single quotes for POSIX shell, escaping any embedded single quotes.
    return "'" .. s:gsub("'", "'\\''") .. "'"
end

local function runex_expand_token(token)
    if not RUNEX_KNOWN[token] or not runex_is_safe_token(token) then
        return token
    end

    local command = runex_shell_quote(RUNEX_BIN) .. ' expand --token=' .. runex_shell_quote(token) .. ' 2>&1'
    local handle = io.popen(command)
    if not handle then
        return token
    end

    local expanded = handle:read("*a")
    handle:close()
    if not expanded then
        return token
    end
    expanded = expanded:gsub("%s+$", "")
    if expanded == "" then
        return token
    end
    -- Split on unit separator (\x1f) for cursor placeholder
    local sep = expanded:find("\x1f")
    if sep then
        local text = expanded:sub(1, sep - 1)
        local cursor_offset = tonumber(expanded:sub(sep + 1))
        return text, cursor_offset
    end
    return expanded, nil
end

function runex_expand(rl_buffer, line_state)
    local line = rl_buffer:getbuffer()
    local cursor = rl_buffer:getcursor()
    local length = rl_buffer:getlength()

    if cursor <= length then
        local ch = line:sub(cursor, cursor)
        if ch ~= " " then
            rl_buffer:insert(" ")
            return
        end
    end

    local left = line:sub(1, cursor - 1)
    local token = left:match("([^ ]+)$")
    if token then
        local token_start = cursor - #token
        local prefix = line:sub(1, token_start - 1)
        if runex_is_command_position(prefix) and RUNEX_KNOWN[token] then
            local expanded, cursor_offset = runex_expand_token(token)
            if expanded ~= token then
                rl_buffer:beginundogroup()
                rl_buffer:remove(token_start, cursor)
                rl_buffer:setcursor(token_start)
                rl_buffer:insert(expanded)
                if cursor_offset then
                    rl_buffer:setcursor(token_start + cursor_offset)
                end
                rl_buffer:endundogroup()
            end
        end
    end

    rl_buffer:insert(" ")
end

rl.describemacro([["luafunc:runex_expand"]], "Expand a runex abbreviation and insert a space")
{CLINK_BINDING}
