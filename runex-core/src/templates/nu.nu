# runex shell integration for Nushell

# Pre-compute command existence cache using shell-native detection (which).
# Detects PATH binaries and Nushell custom commands/aliases.
let __runex_cmds = (do { {NU_BIN} precache --shell nu --list-commands } | complete | get stdout | str trim)
if ($__runex_cmds | is-not-empty) {
    let __runex_resolved = (
        $__runex_cmds
        | split row ','
        | each {|c| $"($c)=(if (which $c | is-not-empty) { 1 } else { 0 })" }
        | str join ','
    )
    let __runex_export = (do { {NU_BIN} precache --shell nu --resolved $__runex_resolved } | complete | get stdout | str trim)
    if ($__runex_export | is-not-empty) {
        # Parse "$env.RUNEX_CMD_CACHE_V1 = '<json>'" and set the env var directly.
        let __runex_json = ($__runex_export | parse --regex "'(?P<j>.*)'" | get j.0? | default '')
        if ($__runex_json | is-not-empty) {
            $env.RUNEX_CMD_CACHE_V1 = $__runex_json
        }
    }
}

$env.config.keybindings = (
    $env.config.keybindings
    | where name != "runex_expand"
    | where name != "runex_self_insert"{NU_BINDINGS}{NU_SELF_INSERT_BINDINGS}
)
