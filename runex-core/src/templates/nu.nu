# runex shell integration for Nushell
# Pre-compute command existence cache using shell-native detection.
# Add to your env.nu:
#   let cmds = (runex precache --shell nu --list-commands | split row ',')
#   let resolved = ($cmds | each {|c| $"($c)=(if (which $c | is-not-empty) { 1 } else { 0 })" } | str join ',')
#   runex precache --shell nu --resolved $resolved | from json | load-env
# Add the following to your $env.config.keybindings:
$env.config.keybindings = (
    $env.config.keybindings
    | where name != "runex_expand"
    | where name != "runex_self_insert"{NU_BINDINGS}{NU_SELF_INSERT_BINDINGS}
)
