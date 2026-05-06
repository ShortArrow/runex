# runex shell integration for Nushell
#
# Known limitation (nu 0.111 / reedline): when a paste contains a
# space that matches the configured trigger key, nu fires the runex
# binding mid-paste. The `executehostcommand` event resets the
# command line at fire time, so any paste content that arrives after
# the triggering space is dropped. Workarounds:
#
#   1. (Recommended) Add a Ctrl+V paste binding that reads the system
#      clipboard directly, sidestepping the per-keystroke trigger:
#        [keybind.paste_intercept]
#        nu = "ctrl-v"
#      The runex binary's `paste-clipboard` subcommand pipes the
#      clipboard text into `commandline edit --insert`, so the abbr
#      trigger never sees individual space keystrokes inside a paste.
#   2. Use a non-space trigger for nu:
#        [keybind.trigger]
#        nu = "shift-space"
#   3. Quote/escape paste content, or paste it in pieces.
#
# This is upstream behaviour, not a runex regression — every keymap
# binding nu invokes inside a paste sees only the prefix received so
# far. The Ctrl+V workaround above is generated only when configured
# in `[keybind.paste_intercept]`.

$env.config.keybindings = (
    $env.config.keybindings
    | where name != "runex_expand"
    | where name != "runex_self_insert"
    | where name != "runex_paste"{NU_BINDINGS}{NU_PASTE_BINDING}{NU_SELF_INSERT_BINDINGS}
)
