Output docs/vhs/demo.gif

Set Shell "bash"
Set FontSize 18
Set Width 1100
Set Height 700
Set TypingSpeed 70ms
Set Padding 24

Type 'clear'
Enter
Sleep 300ms

Type '# create config'
Enter
Sleep 800ms

Type 'cat > /tmp/runex-demo.toml <<EOF'
Enter
Type 'version = 1'
Enter
Type ''
Enter
Type '[keybind]'
Enter
Type 'trigger = "space"'
Enter
Type ''
Enter
Type '[[abbr]]'
Enter
Type 'key = "gcm"'
Enter
Type 'expand = "git commit -m"'
Enter
Type ''
Enter
Type '[[abbr]]'
Enter
Type 'key = "rx"'
Enter
Type 'expand = "runex"'
Enter
Type ''
Enter
Type '[[abbr]]'
Enter
Type 'key = "ll"'
Enter
Type 'expand = "lsd -l"'
Enter
Type ''
Enter
Type '[[abbr]]'
Enter
Type 'key = "ll."'
Enter
Type 'expand = "lsd -la"'
Enter
Type 'EOF'
Enter
Sleep 800ms

Type 'clear'
Enter
Sleep 300ms

Type '# load shell integration'
Enter
Sleep 800ms

Type "export RUNEX_CONFIG=/tmp/runex-demo.toml"
Enter
Sleep 500ms

Type 'runex export bash > /tmp/runex-demo.sh'
Enter
Sleep 500ms

Type 'source /tmp/runex-demo.sh'
Enter
Sleep 800ms

Type 'clear'
Enter
Sleep 300ms

Type '# expand abbreviations'
Enter
Sleep 800ms

Type "g"
Sleep 300ms
Type "c"
Sleep 300ms
Type "m"
Sleep 300ms
Type " "
Sleep 1500ms
Enter
Sleep 1000ms

Type "l"
Sleep 300ms
Type "l"
Sleep 300ms
Type " "
Sleep 1200ms
Enter
Sleep 1000ms

Type "l"
Sleep 300ms
Type "l"
Sleep 300ms
Type "."
Sleep 300ms
Type " "
Sleep 1200ms
Enter
Sleep 1000ms

Type "r"
Sleep 300ms
Type "x"
Sleep 300ms
Type " "
Sleep 1200ms
