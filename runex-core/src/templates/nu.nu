# runex shell integration for Nushell
# Pre-compute command existence cache
# Note: Nushell env vars set in `config.nu` are available to subprocesses.
# Users should add to their env.nu:
#   runex precache --shell nu | from json | load-env
# Add the following to your $env.config.keybindings:
$env.config.keybindings = (
    $env.config.keybindings
    | where name != "runex_expand"
    | where name != "runex_self_insert"{NU_BINDINGS}{NU_SELF_INSERT_BINDINGS}
)
