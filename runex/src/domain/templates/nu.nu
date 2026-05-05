# runex shell integration for Nushell
$env.config.keybindings = (
    $env.config.keybindings
    | where name != "runex_expand"
    | where name != "runex_self_insert"{NU_BINDINGS}{NU_SELF_INSERT_BINDINGS}
)
