# runex shell integration for bash
__runex_expand() {
    local out
    if out=$({BASH_BIN} hook --shell bash --line "$READLINE_LINE" --cursor "$READLINE_POINT" 2>/dev/null) && [ -n "$out" ]; then
        eval "$out"
    else
        READLINE_LINE="${READLINE_LINE:0:READLINE_POINT} ${READLINE_LINE:READLINE_POINT}"
        READLINE_POINT=$((READLINE_POINT + 1))
    fi
}
__runex_self_insert() {
    READLINE_LINE="${READLINE_LINE:0:READLINE_POINT} ${READLINE_LINE:READLINE_POINT}"
    READLINE_POINT=$((READLINE_POINT + 1))
}
{BASH_BIND_LINES}
{BASH_SELF_INSERT_LINES}
