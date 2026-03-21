# runex shell integration for Nushell
# Add the following to your $env.config.keybindings:
$env.config.keybindings = ($env.config.keybindings | append {
    name: runex_expand
    modifier: {NU_MODIFIER}
    keycode: {NU_KEYCODE}
    mode: [emacs vi_insert]
    event: {
        send: executehostcommand
        cmd: "
            let line = (commandline)
            let cursor = (commandline get-cursor)
            if $cursor < ($line | str length) {
                commandline edit --insert ' '
            } else {
                let token = ($line | split row ' ' | first)
                let expanded = ({BIN} expand $"--token=($token)" | complete | get stdout)
                if $expanded != $token {
                    commandline edit --replace ($expanded + ($line | str substring ($token | str length)..))
                }
                commandline edit --append ' '
            }
        "
    }
} | append {
    name: runex_literal_space
    modifier: {NU_LITERAL_MODIFIER}
    keycode: {NU_LITERAL_KEYCODE}
    mode: [emacs vi_insert]
    event: {
        edit: insertchar
        value: ' '
    }
})
