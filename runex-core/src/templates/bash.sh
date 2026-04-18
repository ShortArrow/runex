# runex shell integration for bash
__runex_cmds=$({BASH_BIN} precache --shell bash --list-commands 2>/dev/null)
if [ -n "$__runex_cmds" ]; then
    __runex_resolved=""
    IFS=',' read -ra __runex_arr <<< "$__runex_cmds"
    for __runex_c in "${__runex_arr[@]}"; do
        if command -v "$__runex_c" >/dev/null 2>&1; then
            __runex_resolved="${__runex_resolved:+$__runex_resolved,}${__runex_c}=1"
        else
            __runex_resolved="${__runex_resolved:+$__runex_resolved,}${__runex_c}=0"
        fi
    done
    eval "$({BASH_BIN} precache --shell bash --resolved "$__runex_resolved" 2>/dev/null)"
    unset __runex_cmds __runex_arr __runex_c __runex_resolved
else
    eval "$({BASH_BIN} precache --shell bash 2>/dev/null)"
fi
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
        trap - DEBUG
        PROMPT_COMMAND=
        local raw
        raw=$({BASH_BIN} expand --token="$token" 2>/dev/null)
        # Split on \x1f: text\x1fcursor_offset (or just text if no placeholder)
        if [[ "$raw" == *$'\x1f'* ]]; then
            expanded="${raw%%$'\x1f'*}"
            local cursor_offset="${raw#*$'\x1f'}"
        else
            expanded="$raw"
            local cursor_offset=""
        fi
        if [ "$expanded" != "$token" ]; then
            READLINE_LINE="${prefix}${expanded}${suffix}"
            if [ -n "$cursor_offset" ]; then
                READLINE_POINT=$((token_start + cursor_offset))
            else
                READLINE_POINT=$((token_start + ${#expanded}))
            fi
        fi
    fi

    READLINE_LINE="${READLINE_LINE:0:READLINE_POINT} ${READLINE_LINE:READLINE_POINT}"
    READLINE_POINT=$((READLINE_POINT + 1))
    PROMPT_COMMAND="$runex_prompt_command"
}
{BASH_BIND_LINES}
{BASH_SELF_INSERT_LINES}
