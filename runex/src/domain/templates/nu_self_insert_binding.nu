 | append {
    name: runex_self_insert
    modifier: {NU_SI_MODIFIER}
    keycode: {NU_SI_KEYCODE}
    mode: [emacs vi_insert]
    event: { edit: insertchar, value: ' ' }
}
