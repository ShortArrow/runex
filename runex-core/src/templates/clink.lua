-- runex shell integration for clink
local RUNEX_BIN = {CLINK_BIN}
local RUNEX_KNOWN = {
{CLINK_KNOWN_CASES}
}
local RUNEX_DEBUG_ENABLED = os.getenv("RUNEX_CLINK_DEBUG") == "1"
local RUNEX_DEBUG_LOG = os.getenv("LOCALAPPDATA") and (os.getenv("LOCALAPPDATA") .. "\\clink\\runex.debug.log") or nil
local RUNEX_STARTUP_LOG = os.getenv("LOCALAPPDATA") and (os.getenv("LOCALAPPDATA") .. "\\clink\\runex.startup.log") or nil

local function runex_write_log(path, message)
    if not path then
        return
    end

    local file = io.open(path, "a")
    if not file then
        return
    end

    file:write(os.date("%Y-%m-%d %H:%M:%S"), " ", message, "\n")
    file:close()
end

local function runex_debug_log(message)
    if not RUNEX_DEBUG_ENABLED or not RUNEX_DEBUG_LOG then
        return
    end

    runex_write_log(RUNEX_DEBUG_LOG, message)
end

runex_write_log(RUNEX_STARTUP_LOG, "startup localappdata=" .. tostring(os.getenv("LOCALAPPDATA")) .. " debug=" .. tostring(RUNEX_DEBUG_ENABLED))

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
        runex_debug_log("expand_token skip token=" .. tostring(token))
        return token
    end

    local command = '"' .. RUNEX_BIN .. '" expand --token=' .. token .. ' 2>&1'
    runex_debug_log("expand_token command=" .. command)
    local handle = io.popen(command)
    if not handle then
        runex_debug_log("expand_token popen_failed token=" .. tostring(token))
        return token
    end

    local expanded = handle:read("*a")
    handle:close()
    if not expanded then
        runex_debug_log("expand_token read_nil token=" .. tostring(token))
        return token
    end
    expanded = expanded:gsub("%s+$", "")
    if expanded == "" then
        runex_debug_log("expand_token empty token=" .. tostring(token))
        return token
    end
    runex_debug_log("expand_token token=" .. tostring(token) .. " expanded=" .. tostring(expanded))
    return expanded
end

function runex_expand(rl_buffer, line_state)
    local line = rl_buffer:getbuffer()
    local cursor = rl_buffer:getcursor()
    local length = rl_buffer:getlength()
    runex_debug_log("expand line=" .. tostring(line) .. " cursor=" .. tostring(cursor) .. " length=" .. tostring(length))

    if cursor <= length then
        local ch = line:sub(cursor, cursor)
        if ch ~= " " then
            runex_debug_log("expand midline_insert ch=" .. tostring(ch))
            rl_buffer:insert(" ")
            return
        end
    end

    local left = line:sub(1, cursor - 1)
    local token = left:match("([^ ]+)$")
    if token then
        local token_start = cursor - #token
        local prefix = line:sub(1, token_start - 1)
        runex_debug_log("expand token=" .. tostring(token) .. " prefix=" .. tostring(prefix))
        if runex_is_command_position(prefix) and RUNEX_KNOWN[token] then
            local expanded = runex_expand_token(token)
            if expanded ~= token then
                runex_debug_log("expand apply token=" .. tostring(token) .. " expanded=" .. tostring(expanded))
                rl_buffer:beginundogroup()
                rl_buffer:remove(token_start, cursor)
                rl_buffer:setcursor(token_start)
                rl_buffer:insert(expanded)
                rl_buffer:endundogroup()
            end
        else
            runex_debug_log("expand skip token=" .. tostring(token) .. " command_pos=" .. tostring(runex_is_command_position(prefix)) .. " known=" .. tostring(RUNEX_KNOWN[token]))
        end
    end

    rl_buffer:insert(" ")
end

rl.describemacro([["luafunc:runex_expand"]], "Expand a runex abbreviation and insert a space")
{CLINK_BINDING}
