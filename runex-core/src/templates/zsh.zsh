# runex shell integration for zsh
__runex_cmds=$({ZSH_BIN} precache --shell zsh --list-commands 2>/dev/null)
if [[ -n "$__runex_cmds" ]]; then
    __runex_resolved=""
    for __runex_c in ${(s:,:)__runex_cmds}; do
        if (( $+commands[$__runex_c] )) || whence -w "$__runex_c" >/dev/null 2>&1; then
            __runex_resolved="${__runex_resolved:+$__runex_resolved,}${__runex_c}=1"
        else
            __runex_resolved="${__runex_resolved:+$__runex_resolved,}${__runex_c}=0"
        fi
    done
    eval "$({ZSH_BIN} precache --shell zsh --resolved "$__runex_resolved" 2>/dev/null)"
    unset __runex_cmds __runex_c __runex_resolved
else
    eval "$({ZSH_BIN} precache --shell zsh 2>/dev/null)"
fi
function __runex_trim_trailing_spaces() {
    local s="$1"
    while [[ "$s" == *" " ]]; do
        s="${s% }"
    done
    printf '%s' "$s"
}

function __runex_is_command_position() {
    local prefix
    prefix="$(__runex_trim_trailing_spaces "$1")"

    if [[ -z "$prefix" ]]; then
        return 0
    fi

    case "$prefix" in
        *'&&'|*'||'|*'|'|*';')
            return 0
            ;;
    esac

    local prev_word="${prefix##* }"
    if [[ "$prev_word" == "sudo" ]]; then
        local before_sudo="${prefix%sudo}"
        before_sudo="$(__runex_trim_trailing_spaces "$before_sudo")"
        if [[ -z "$before_sudo" ]]; then
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

function __runex_is_known_token() {
    case "$1" in
{ZSH_KNOWN_CASES}
    esac
}

function __runex_expand_buffer() {
    local left="$LBUFFER"
    local right="$RBUFFER"

    if [[ -n "$right" && "${right[1,1]}" != " " ]]; then
        LBUFFER+=" "
        return
    fi

    local token="${left##* }"
    if [[ -n "$token" ]]; then
        local prefix="${left[1,$(( ${#left} - ${#token} ))]}"
        if ! __runex_is_command_position "$prefix"; then
            LBUFFER+=" "
            return
        fi
        if ! __runex_is_known_token "$token"; then
            LBUFFER+=" "
            return
        fi
        local raw
        raw=$({ZSH_BIN} expand --token="$token" 2>/dev/null)
        local expanded cursor_offset
        if [[ "$raw" == *$'\x1f'* ]]; then
            expanded="${raw%%$'\x1f'*}"
            cursor_offset="${raw#*$'\x1f'}"
        else
            expanded="$raw"
            cursor_offset=""
        fi
        if [[ "$expanded" != "$token" ]]; then
            LBUFFER="${prefix}${expanded}"
            if [[ -n "$cursor_offset" ]]; then
                # LBUFFER length determines cursor; set it to prefix + offset
                local full="${prefix}${expanded} "
                LBUFFER="${full:0:$((${#prefix} + cursor_offset))}"
                RBUFFER="${full:$((${#prefix} + cursor_offset))}"
                return
            fi
        fi
    fi

    LBUFFER+=" "
}

function __runex_expand() {
    __runex_expand_buffer
    zle redisplay
}

zle -N __runex_expand
{ZSH_BIND_LINES}
{ZSH_SELF_INSERT_LINES}
