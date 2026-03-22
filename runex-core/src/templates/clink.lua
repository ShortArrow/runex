-- runex shell integration for clink
local RUNEX_BIN = {CLINK_BIN}
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
    return token:match("^[%w%._%-]+$") ~= nil
end

local function runex_expand_token(token)
    if not RUNEX_KNOWN[token] or not runex_is_safe_token(token) then
        return token
    end

    local command = '"' .. RUNEX_BIN .. '" expand --token=' .. token .. ' 2>&1'
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
    return expanded
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
            local expanded = runex_expand_token(token)
            if expanded ~= token then
                rl_buffer:beginundogroup()
                rl_buffer:remove(token_start, cursor)
                rl_buffer:setcursor(token_start)
                rl_buffer:insert(expanded)
                rl_buffer:endundogroup()
            end
        end
    end

    rl_buffer:insert(" ")
end

rl.describemacro([["luafunc:runex_expand"]], "Expand a runex abbreviation and insert a space")
{CLINK_BINDING}
