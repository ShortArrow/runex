# runex shell integration for Nushell
#
# Known limitation (nu 0.111 / reedline): when a paste contains a
# space that matches the configured trigger key, nu fires the runex
# binding mid-paste. The `executehostcommand` event resets the
# command line at fire time, so any paste content that arrives after
# the triggering space is dropped. Workarounds:
#
#   1. Use a non-space trigger for nu by setting in your config.toml:
#        [keybind.trigger]
#        nu = "shift+space"
#   2. Quote/escape paste content, or paste it in pieces.
#
# This is upstream behaviour, not a runex regression — every keymap
# binding nu invokes inside a paste sees only the prefix received so
# far. See https://github.com/nushell/nushell/issues/ for tracking;
# the runex issue tracker links the relevant upstream report.

$env.config.keybindings = (
    $env.config.keybindings
    | where name != "runex_expand"
    | where name != "runex_self_insert"{NU_BINDINGS}{NU_SELF_INSERT_BINDINGS}
)
