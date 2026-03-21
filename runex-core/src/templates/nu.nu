# runex shell integration for Nushell
# Add the following to your $env.config.keybindings:
$env.config.keybindings = (
    $env.config.keybindings
    | where name != "runex_expand"{NU_BINDINGS}
)
