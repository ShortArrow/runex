 | append {
    name: runex_expand
    modifier: {NU_MODIFIER}
    keycode: {NU_KEYCODE}
    mode: [emacs vi_insert]
    event: {
        send: executehostcommand
        cmd: "
            let line = (commandline)
            let cursor = (commandline get-cursor)
            let out = ({NU_BIN} hook --shell nu --line $line --cursor $cursor | complete | get stdout | str trim --right)
            if ($out | is-empty) {
                commandline edit --insert ' '
            } else {
                let r = ($out | from json)
                commandline edit --replace $r.line
                commandline set-cursor $r.cursor
            }
        "
    }
}
