# runex shell integration for bash
__runex_expand() {
    local left="${READLINE_LINE:0:READLINE_POINT}"
    local right="${READLINE_LINE:READLINE_POINT}"

    if [ -n "$right" ] && [ "${right:0:1}" != " " ]; then
        READLINE_LINE="${left} ${right}"
        READLINE_POINT=$((READLINE_POINT + 1))
        return
    fi

    local token="${left##* }"
    if [ -n "$token" ]; then
        local token_start=$((READLINE_POINT - ${#token}))
        local prefix="${READLINE_LINE:0:token_start}"
        local suffix="${READLINE_LINE:READLINE_POINT}"
        local expanded
        expanded=$({BIN} expand --token "$token" 2>/dev/null)
        if [ "$expanded" != "$token" ]; then
            READLINE_LINE="${prefix}${expanded}${suffix}"
            READLINE_POINT=$((token_start + ${#expanded}))
        fi
    fi

    READLINE_LINE="${READLINE_LINE:0:READLINE_POINT} ${READLINE_LINE:READLINE_POINT}"
    READLINE_POINT=$((READLINE_POINT + 1))
}
bind -x '"\x20": __runex_expand'
