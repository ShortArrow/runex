# runex shell integration for bash
__runex_expand() {
    local token="${READLINE_LINE%% *}"
    if [ -n "$token" ]; then
        local expanded
        expanded=$({BIN} expand --token "$token" 2>/dev/null)
        if [ "$expanded" != "$token" ]; then
            READLINE_LINE="${expanded}${READLINE_LINE#$token}"
            READLINE_POINT=${#expanded}
        fi
    fi
    READLINE_LINE="${READLINE_LINE} "
    READLINE_POINT=$((READLINE_POINT + 1))
}
bind -x '"\x20": __runex_expand'
