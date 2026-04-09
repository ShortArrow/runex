Output "/out/demo.gif"

Set Shell "bash"
Set FontSize 20
Set Width 900
Set Height 400
Set TypingSpeed 80ms
Set Padding 24

# Setup (hidden from recording)
Hide

Type 'cat > /tmp/runex-demo.toml <<EOF'
Enter
Type 'version = 1'
Enter
Type '[keybind.trigger]'
Enter
Type 'default = "space"'
Enter
Type '[[abbr]]'
Enter
Type 'key = "gcm"'
Enter
Type 'expand = "git commit -m"'
Enter
Type '[[abbr]]'
Enter
Type 'key = "rx"'
Enter
Type 'expand = "runex"'
Enter
Type 'when_command_exists = ["runex"]'
Enter
Type '[[abbr]]'
Enter
Type 'key = "rxd"'
Enter
Type 'expand = "runex doctor"'
Enter
Type 'when_command_exists = ["runex"]'
Enter
Type '[[abbr]]'
Enter
Type 'key = "rxt"'
Enter
Type 'expand = "runex timings"'
Enter
Type 'when_command_exists = ["runex"]'
Enter
Type '[[abbr]]'
Enter
Type 'key = "ls."'
Enter
Type 'expand = "ls -a"'
Enter
Type '[[abbr]]'
Enter
Type 'key = "ll"'
Enter
Type 'expand = "ls -l"'
Enter
Type '[[abbr]]'
Enter
Type 'key = "ll."'
Enter
Type 'expand = "ls -la"'
Enter
Type 'EOF'
Enter
Sleep 500ms

Type "export RUNEX_CONFIG=/tmp/runex-demo.toml"
Enter
Sleep 300ms

Type 'eval "$(runex export bash)"'
Enter
Sleep 500ms

Type 'clear'
Enter
Sleep 300ms

Show

# Demo: abbreviation expansion
Sleep 500ms

# gcm -> git commit -m
Type "g"
Sleep 100ms
Type "c"
Sleep 100ms
Type "m"
Sleep 1000ms
Type " "
Sleep 1000ms
Enter
Sleep 500ms

# ll -> ls -l
Type "l"
Sleep 100ms
Type "l"
Sleep 1000ms
Type " "
Sleep 1000ms
Enter
Sleep 500ms

# ll. -> ls -la
Type "l"
Sleep 100ms
Type "l"
Sleep 100ms
Type "."
Sleep 1000ms
Type " "
Sleep 1000ms
Enter
Sleep 500ms

# ls. -> ls -a
Type "l"
Sleep 100ms
Type "s"
Sleep 100ms
Type "."
Sleep 1000ms
Type " "
Sleep 1000ms
Enter
Sleep 500ms

# rx -> runex
Type "r"
Sleep 100ms
Type "x"
Sleep 1000ms
Type " "
Sleep 1000ms
Enter
Sleep 500ms

# rxd -> runex doctor
Type "r"
Sleep 100ms
Type "x"
Sleep 100ms
Type "d"
Sleep 1000ms
Type " "
Sleep 1000ms
Type "--no-shell-aliases"
Enter
Sleep 2000ms

# rxt -> runex timings
Type "r"
Sleep 100ms
Type "x"
Sleep 100ms
Type "t"
Sleep 1000ms
Type " "
Sleep 1000ms
Type "rx"
Enter
Sleep 2000ms
