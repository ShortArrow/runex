# Design intent

Why runex is built the way it is, while the reasons hold. Each section states what the design protects, the forces in tension, what it refuses, and how the next case of the same kind is decided. When a force changes, the section changes with it.

## Shell integration is a static cache

`runex init <shell>` writes the shell integration to a cache file and adds one line to the rc file that sources it by absolute path. clink has no rc file: `runex init clink` writes the lua file into clink's script directory instead. The cache calls the runex binary by the absolute path of whichever runex wrote it, `init`, `add`, `remove` or `config reload`.

**What it protects.** The hook runs on every press of the trigger key, so anything paid per key press is paid hundreds of times a session. Resolving `runex` through `PATH` is such a cost: behind a mise shim the call took 0.474 s against 0.002 s for the binary itself (`time runex hook --shell bash --line ls --cursor 2`, WSL Arch Linux, 2026-05-10). With the path baked into the cache, neither a key press nor shell start resolves `runex` through `PATH`, and shell start does not run runex at all.

**Forces.** A baked path is specific to one machine, while rc files are often synced between machines in a dotfiles repository. The cache keeps the machine-specific path out of the rc file: the rc line names a cache path, and each machine writes its own cache. A user who wants `PATH` resolution can still run `runex export <shell> --bin runex`.

**Invariants.**

- The cache lives at `<XDG_CACHE_HOME>/runex/integration.<ext>` for bash, zsh, pwsh and nu, and at `%LOCALAPPDATA%\clink\runex.lua` for clink.
- The first line of a bash, zsh, pwsh or nu cache carries `runex-integration-version: <N>`. A change to the shape of the cache body bumps `INTEGRATION_CACHE_VERSION`, and `runex doctor` then reports every older cache as outdated. Moving the cache also bumps it, and doctor then tells the user where the old cache was.
- The clink lua file has no version header. `runex doctor` compares it with the output of `runex export clink` and reports any difference.
- The cache returns early when sourced by a non-interactive shell, so scripts that inherit the rc file bind no keys.
- `runex add`, `runex remove` and `runex config reload` rewrite the bash, zsh, pwsh and nu caches that already exist, and never the clink file, which `runex init clink` rewrites.
- For `add` and `remove` the rewrite is best effort: a failure is a warning on stderr and never changes their exit status, and a config that does not load still produces caches, with no rules.
- `config reload` exists to apply a hand edit, so it is strict: a config that does not load leaves every cache untouched and exits 1, and a failed rewrite exits 1.
- `runex doctor` reports drift and says how to fix it. It never rewrites a cache itself, because a stale cache may be deliberate, for example while a template is being debugged.

**Refused.**

- Writing the absolute path into the rc file. It breaks rc files synced between machines, and after a reinstall the rc file keeps calling the old path.
- `eval "$(runex export <shell>)"` at shell start as the installed form. Every shell start pays one `PATH` lookup and one process.
- Binding lazily at the first prompt. Each shell would need a different mechanism (bash `PROMPT_COMMAND`, zsh `precmd`, none in PSReadLine), and the install would stop being one contract for all shells.

**The next shell.** A new shell gets a template, a cache path in `infra::integration_cache::cache_path`, and an `init` entry; `add`, `remove`, `config reload` and `doctor` then cover it.

## The buffer travels as hex where a host rewrites arguments

bash, zsh and nu pass the buffer as one argument joined to its option, `--line=<text>`. clink and PowerShell pass hex of its UTF-8 bytes, `--line-hex`.

**What it protects.** Every character a user can type expands the same way on every shell. A transport that drops or changes a character turns the trigger key into a plain space, with no message.

**Forces.** A buffer can start with `-`. Passed as a separate argument it would be read as an option, so every template joins the value to its option with `=`.

Two hosts also rewrite argument text before runex receives it:

- clink reaches runex only through `io.popen`, which runs a cmd.exe command line. cmd.exe has no escape for `"`, expands `%VAR%` even inside quotes, and expands `!VAR!` under delayed expansion.
- PowerShell rewrites an argument that starts with `~` to the home directory, and Windows PowerShell 5.1 re-quotes arguments so that one containing `"` is split again by the child process.

Hex uses only `[0-9A-F]`, which neither host rewrites. Its cost is size: hex doubles the buffer, and cmd.exe runs at most 8158 characters after `cmd /c` (measured on Windows 11). The clink template therefore measures the assembled command against `CMD_LINE_MAX = 8000` and inserts a plain space above it, which caps clink expansion at a buffer of about 3900 bytes.

**Details each host forces.**

- cmd.exe collapses an empty argument into a missing one, so the clink template does not call runex for an empty buffer.
- Windows PowerShell 5.1 drops a separate empty argument, so the pwsh template joins the value to the option, `--line-hex=<HEX>`, and an empty buffer still arrives.
- The pwsh template computes the hex inline on the `$hookArgs = @(` line, the one expression both PowerShell hosts accept. `runex/tests/pwsh_integration.rs` evaluates that line from the exported bootstrap, so the tests run the template's real argument form.

**Refused.**

- Escaping for the host's parser (doubling `"`, escaping backslashes before quotes). It depends on undocumented corner cases of two parsers, and on PowerShell it is right for one host and wrong for the other.
- Escaping only the unsafe characters. The unsafe set keeps growing (`"`, `%`, `!`, `^`, `&`, `|`, `<`, `>`, control characters, the console code page).
- Passing the buffer in an environment variable or a temporary file. The variable mutates the clink process and cmd.exe still expands it if referenced; the file adds I/O and cleanup on every key press.
- `$PSNativeCommandArgumentPassing` and `--%`. The first does not exist in Windows PowerShell 5.1; the second also stops `$line` and `$cursor` from expanding.

**The next shell.** A shell whose integration reaches runex without re-quoting uses `--line=`. A shell that goes through a layer which rewrites argument text uses `--line-hex`.

## Pattern keys are opt-in

A rule's `key` matches literally, or with `{number}`, unless the rule sets `match = "glob"` or `match = "regex"`.

**What it protects.** A key may already contain `*`, `?`, `.` or `^` and match it literally. Giving those characters a meaning inside every `key` would silently change existing configs, so the meaning is switched on per rule.

**Precedence.** Exact rules first, then `{number}`, then glob, then regex; within one kind the first rule in the file wins. An exact key always beats a pattern, which is how a user carves a special case out of a broad rule.

**Forces.**

- Git Bash expands from tables baked into the cache, matching in bash itself. A glob key is handed to a bash `case` pattern, so it is limited to `*` and `?`, with at least one wildcard and at most one `*`, and `[`, `]`, `{`, `}`, `(`, `)` and `\` are rejected: in a `case` pattern they would mean what the Rust matcher does not.
- bash's `=~` rejects `\d`, named groups, `(?:..)` and lazy quantifiers, so regex rules are left out of the Git Bash tables and `runex doctor` says so on Windows. Calling `runex` from the baked path instead is refused, because spawning a Win32 executable from that path makes the shell lose the next Ctrl+C.
- The hook re-reads the config on every key press, so compiling the regex keys is paid on every press. Regex keys use `regex-lite`: compiling 50 typical patterns took 0.14 ms against 15.0 ms with `regex`, and it adds 0.10 MB to the binary against 1.58 MB (Windows 11, release build, 50 runs, medians, 2026-10-01). Its `\w`, `\d` and case-insensitivity are ASCII-only, which abbreviation tokens almost always are.
- A regex key matches the whole token (`^(?:key)$`), so a short pattern does not fire on every token containing it. It must compile both alone and anchored, may have at most 16 capture groups, and may compile to at most 64 KiB (`MAX_REGEX_GROUPS`, `REGEX_SIZE_LIMIT` in `domain/expand.rs`): the engine's own limit bounds only the automaton, and a 915-byte key once made every key press allocate about 2.3 GiB for capture slots.

**Captures.** In `expand`, `{*}` inserts what a glob's `*` matched, and `{1}` to `{9}` and `{name}` insert a regex key's groups; a group that did not take part inserts nothing. `{N}` beyond the key's group count is a config error. `{name}` that names no group stays literal, so shell braces such as `awk '{print}'` survive.

**Self-loops.** A glob or regex rule whose rendering equals the token is skipped, like an exact rule whose `expand` equals its `key`. A broad pattern would otherwise rewrite a token to itself and hide every later rule of its kind.

**Refused.**

- A prefix inside `key` (`glob:g*`). A key that already starts with the prefix would change meaning, and the prefix is easy to miss when reading a config.
- Separate `glob` and `regex` fields. Every consumer of `key` (`list`, `which`, `doctor`, the Git Bash tables) would need two more names for the same role.
- Precedence by file order alone. It would drop the guarantee that an exact key wins.
- Full glob syntax (`[abc]`, `**`, braces). Tokens have no path structure, and each construct would have to be reproduced in the Git Bash tables.

## A rule that renders blank is skipped

A rule whose expansion is empty or whitespace once `{}` is removed is skipped, in every phase, like a rule whose `when_command_exists` fails: the next rule with a matching key is tried, and otherwise the trigger key inserts a plain space. `runex which --why` reports the skip and `runex doctor` warns about a rule that is blank for every token.

**What it protects.** An abbreviation never deletes what the user typed. `expand = ""` is rejected when the config loads for this reason; `"{}"` reaches the same result through a spelling that check cannot see.

**Refused.**

- Firing the rule. Deleting a word is the job of the shell's editing keys.
- Rejecting the rule when the config loads. Validation fails the whole file, so one such rule would stop every rule on every shell.

On Git Bash, whether a `{number}` rule is blank can depend on the token, because a repeated unit can form `{}`. The baked tables keep every rule and test the rendering at key press time against the same whitespace set as Rust's `str::trim`.

## Linux CI runs in one digest-pinned image

The `test-linux` job runs in `ghcr.io/shortarrow/runex-ci`, built from `containers/ci/ubuntu.Dockerfile` and named in `ci.yml` by digest. Developers can run that digest locally to reproduce a CI run; the `:latest` tag CONTRIBUTING suggests for everyday runs is the most recent build, which can be newer than the pinned one.

**What it protects.** A Linux CI failure can be reproduced by running the pinned image, and the shells the tests drive (bash, zsh, pwsh, nu) and the clipboard tools change only in a reviewed commit. On a hosted runner image these move whenever the image is respun.

**Invariants.**

- The base image is pinned by digest, and the tool versions are `ARG` defaults (`NU_VERSION`, `RUST_TOOLCHAIN`, `NODE_MAJOR`), so a bump is a commit.
- `containers/ci/sanity.sh` runs as the last `RUN`, so a missing tool fails the image build, not a CI run.
- The image user is uid 1001, matching the `--user 1001` the job passes. `RUSTUP_HOME` and `CARGO_HOME` are set with `ENV`, because GitHub Actions overrides `HOME` inside the container.
- `build-ci-image.yml` builds with `context: containers/ci`, so the `.dockerignore` beside the Dockerfile applies. On a pull request it builds without logging in to GHCR or pushing.
- Every `actions/checkout` step sets `persist-credentials: false`, so the token does not stay on disk for build or test code to read.
- `RUN` steps run under `bash -o pipefail`, so a failed download fails the step even when piped into another command. hadolint checks every Dockerfile in CI.

**Refused.**

- Tracking the image by tag in `ci.yml`. A rebuilt image would change every running pull request's gate without a commit.
- Installing tools on the hosted runner per job. Nothing pins their versions, and developers cannot reproduce the runner.
- Pinning apt package versions. A pinned version disappears from the archive and the image stops building; the digest-pinned base already fixes the starting point (hadolint DL3008 is off in `.hadolint.yaml` for this reason).
