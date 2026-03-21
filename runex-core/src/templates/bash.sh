# runex shell integration for bash
__runex_expand() {
    local runex_prompt_command="${PROMPT_COMMAND-}"
    local runex_restore_prompt_command
    printf -v runex_restore_prompt_command 'PROMPT_COMMAND=%q' "$runex_prompt_command"
    local runex_ps0="${PS0-}"
    local runex_restore_ps0
    printf -v runex_restore_ps0 'PS0=%q' "$runex_ps0"

    local left="${READLINE_LINE:0:READLINE_POINT}"
    local right="${READLINE_LINE:READLINE_POINT}"

    if [ -n "$right" ] && [ "${right:0:1}" != " " ]; then
        READLINE_LINE="${left} ${right}"
        READLINE_POINT=$((READLINE_POINT + 1))
        PROMPT_COMMAND="$runex_restore_prompt_command"
        eval "$runex_restore_ps0"
        return
    fi

    local token="${left##* }"
    if [ -n "$token" ]; then
        local token_start=$((READLINE_POINT - ${#token}))
        local prefix="${READLINE_LINE:0:token_start}"
        local suffix="${READLINE_LINE:READLINE_POINT}"
        local expanded
        local runex_debug_trap
        runex_debug_trap="$(trap -p DEBUG)"
        trap - DEBUG
        PS0=
        PROMPT_COMMAND=
        expanded=$({BIN} expand --token="$token" 2>/dev/null)
        eval "$runex_restore_ps0"
        if [ -n "$runex_debug_trap" ]; then
            eval "$runex_debug_trap"
        fi
        if [ "$expanded" != "$token" ]; then
            READLINE_LINE="${prefix}${expanded}${suffix}"
            READLINE_POINT=$((token_start + ${#expanded}))
        fi
    fi

    READLINE_LINE="${READLINE_LINE:0:READLINE_POINT} ${READLINE_LINE:READLINE_POINT}"
    READLINE_POINT=$((READLINE_POINT + 1))
    PROMPT_COMMAND="$runex_restore_prompt_command"
    eval "$runex_restore_ps0"
}
bind -x '"{BASH_CHORD}": __runex_expand'
