# runex shell integration for bash
__runex_trim_trailing_spaces() {
    local s="$1"
    while [[ "$s" == *" " ]]; do
        s="${s% }"
    done
    printf '%s' "$s"
}

__runex_is_command_position() {
    local prefix
    prefix="$(__runex_trim_trailing_spaces "$1")"

    if [ -z "$prefix" ]; then
        return 0
    fi

    case "$prefix" in
        *'&&'|*'||'|*'|'|*';')
            return 0
            ;;
    esac

    local prev_word="${prefix##* }"
    if [ "$prev_word" = "sudo" ]; then
        local before_sudo="${prefix%sudo}"
        before_sudo="$(__runex_trim_trailing_spaces "$before_sudo")"
        if [ -z "$before_sudo" ]; then
            return 0
        fi
        case "$before_sudo" in
            *'&&'|*'||'|*'|'|*';')
                return 0
                ;;
        esac
    fi

    return 1
}

__runex_is_known_token() {
    case "$1" in
{BASH_KNOWN_CASES}
    esac
}

__runex_expand() {
    local runex_prompt_command="${PROMPT_COMMAND-}"

    local left="${READLINE_LINE:0:READLINE_POINT}"
    local right="${READLINE_LINE:READLINE_POINT}"

    if [ -n "$right" ] && [ "${right:0:1}" != " " ]; then
        READLINE_LINE="${left} ${right}"
        READLINE_POINT=$((READLINE_POINT + 1))
        PROMPT_COMMAND="$runex_prompt_command"
        return
    fi

    local token="${left##* }"
    if [ -n "$token" ]; then
        local token_start=$((READLINE_POINT - ${#token}))
        local prefix="${READLINE_LINE:0:token_start}"
        if ! __runex_is_command_position "$prefix"; then
            READLINE_LINE="${left} ${right}"
            READLINE_POINT=$((READLINE_POINT + 1))
            PROMPT_COMMAND="$runex_prompt_command"
            return
        fi
        if ! __runex_is_known_token "$token"; then
            READLINE_LINE="${left} ${right}"
            READLINE_POINT=$((READLINE_POINT + 1))
            PROMPT_COMMAND="$runex_prompt_command"
            return
        fi
        local suffix="${READLINE_LINE:READLINE_POINT}"
        local expanded
        local runex_debug_trap
        runex_debug_trap="$(trap -p DEBUG)"
        trap - DEBUG
        PROMPT_COMMAND=
        expanded=$({BIN} expand --token="$token" 2>/dev/null)
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
    PROMPT_COMMAND="$runex_prompt_command"
}
{BASH_BIND_LINES}
