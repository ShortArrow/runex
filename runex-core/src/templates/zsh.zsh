# runex shell integration for zsh
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

function __runex_expand() {
    local left="$LBUFFER"
    local right="$RBUFFER"

    if [[ -n "$right" && "${right:0:1}" != " " ]]; then
        LBUFFER+=" "
        zle redisplay
        return
    fi

    local token="${left##* }"
    if [[ -n "$token" ]]; then
        local token_start=$((${#left} - ${#token}))
        local prefix="${left:0:token_start}"
        if ! __runex_is_command_position "$prefix"; then
            LBUFFER+=" "
            zle redisplay
            return
        fi
        if ! __runex_is_known_token "$token"; then
            LBUFFER+=" "
            zle redisplay
            return
        fi
        local expanded
        expanded=$({ZSH_BIN} expand --token="$token" 2>/dev/null)
        if [[ "$expanded" != "$token" ]]; then
            LBUFFER="${prefix}${expanded}"
        fi
    fi

    LBUFFER+=" "
    zle redisplay
}

zle -N __runex_expand
{ZSH_BIND_LINES}
