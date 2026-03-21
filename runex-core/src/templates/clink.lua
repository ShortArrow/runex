-- runex shell integration for clink
local RUNEX_BIN = {CLINK_BIN}
local RUNEX_KNOWN = {
{CLINK_KNOWN_CASES}
}

local function runex_expand(line)
    local token = line:match("^(%S+)")
    if token and RUNEX_KNOWN[token] then
        local handle = io.popen('"' .. RUNEX_BIN .. '" expand "--token=' .. token .. '" 2>nul')
        if handle then
            local expanded = handle:read("*a"):gsub("%s+$", "")
            handle:close()
            if expanded ~= "" and expanded ~= token then
                return expanded .. line:sub(#token + 1)
            end
        end
    end
    return nil
end

{CLINK_REGISTRATION}
