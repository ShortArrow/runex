-- runex shell integration for clink
local function runex_expand(line)
    local token = line:match("^(%S+)")
    if token then
        local handle = io.popen("{BIN} expand --token " .. token .. " 2>nul")
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

clink.onfilterinput(function(line)
    local result = runex_expand(line)
    if result then
        return result
    end
end)
