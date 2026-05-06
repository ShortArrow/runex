 | append {
    name: runex_paste
    modifier: control
    keycode: char_v
    mode: [emacs vi_insert]
    event: {
        send: executehostcommand
        cmd: "
            let clip = ({NU_BIN} paste-clipboard | str join '')
            if not ($clip | is-empty) {
                commandline edit --insert $clip
            }
        "
    }
}
