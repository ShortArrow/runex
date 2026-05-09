# runex shell integration for zsh
# No-op when sourced by a non-interactive shell (`zsh -c`, scripts, CI).
[[ -o interactive ]] || return 0
function __runex_expand() {
    local line="$LBUFFER$RBUFFER"
    local cursor=${#LBUFFER}
    local out
    if out=$({ZSH_BIN} hook --shell zsh --line "$line" --cursor "$cursor" 2>/dev/null) && [[ -n "$out" ]]; then
        eval "$out"
    else
        LBUFFER+=" "
    fi
    zle redisplay
}

function __runex_self_insert() {
    LBUFFER+=" "
    zle redisplay
}

zle -N __runex_expand
zle -N __runex_self_insert
{ZSH_BIND_LINES}
{ZSH_SELF_INSERT_LINES}
