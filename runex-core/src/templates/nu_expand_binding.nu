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
            let line_len = ($line | str length)
            let right = if $cursor >= $line_len {
                ''
            } else {
                ($line | str substring $cursor..($line_len - 1))
            }

            if ($right | str length) > 0 and not ($right | str starts-with ' ') {
                commandline edit --insert ' '
            } else {
                let left = if $cursor == 0 {
                    ''
                } else {
                    ($line | str substring 0..($cursor - 1))
                }
                let left_trim = ($left | str trim --right)

                if $left != $left_trim or ($left_trim | is-empty) {
                    commandline edit --insert ' '
                } else {
                    let token = (($left_trim | split row ' ' | where $it != '' ) | last)
                    let token_start = (($left_trim | str length) - ($token | str length))
                    let prefix = if $token_start == 0 {
                        ''
                    } else {
                        ($left_trim | str substring 0..($token_start - 1))
                    }
                    let prefix_trim = ($prefix | str trim --right)
                    let command_pos = (
                        ($prefix_trim | is-empty)
                        or ($prefix_trim | str ends-with '||')
                        or ($prefix_trim | str ends-with '&&')
                        or ($prefix_trim | str ends-with '|')
                        or ($prefix_trim | str ends-with ';')
                        or ($prefix_trim == 'sudo')
                        or ($prefix_trim | str ends-with '|| sudo')
                        or ($prefix_trim | str ends-with '&& sudo')
                        or ($prefix_trim | str ends-with '| sudo')
                        or ($prefix_trim | str ends-with '; sudo')
                    )

                    if not $command_pos {
                        commandline edit --insert ' '
                    } else {
                        let expanded = ({NU_BIN} expand --token=($token) | complete | get stdout | str trim --right)
                        if $expanded != $token {
                            commandline edit --replace ($prefix + $expanded + $right)
                            commandline set-cursor (($prefix | str length) + ($expanded | str length))
                        }
                        commandline edit --insert ' '
                    }
                }
            }
        "
    }
}
