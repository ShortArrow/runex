//! Interactive y/N prompt + safe rcfile read.
//!
//! Two unrelated helpers grouped here because they share the same
//! "I/O policy with a hard size cap" shape and are both small enough
//! that a separate file per function would be over-engineered.

use std::io;
use std::path::Path;

/// Maximum byte size of an rc file that `init` will read for marker
/// detection. Files larger than this are treated as if the marker is
/// absent so that init fails safe (appends the integration line)
/// rather than consuming unbounded memory.
pub const MAX_RC_FILE_BYTES: usize = 1024 * 1024; // 1 MB

/// Read a shell rc file for `RUNEX_INIT_MARKER` detection.
///
/// Returns an empty string instead of an error when:
/// - the file doesn't exist (init creates it)
/// - the file isn't a regular file (FIFO / device node — refuse to read)
/// - the file exceeds [`MAX_RC_FILE_BYTES`] (safety: never read enormous files)
/// - any I/O error occurs (init falls back to appending the marker)
///
/// On Unix, `O_NOFOLLOW` matches the policy of the write side
/// (`install_rcfile_integration` in `cmd::init`). Without it, the
/// marker check here could decide "already present" by reading
/// through a symlink target, while the write side would refuse to
/// follow and try to append. Both sides agree on "no symlinks at the
/// final path component".
pub fn read_rc_content(path: &Path) -> String {
    use std::io::Read;
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        match std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
        {
            Ok(f) => f,
            Err(_) => return String::new(),
        }
    };
    #[cfg(not(unix))]
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    let meta = match file.metadata() {
        Ok(m) => m,
        Err(_) => return String::new(),
    };
    if !meta.is_file() {
        return String::new();
    }
    if meta.len() > MAX_RC_FILE_BYTES as u64 {
        return String::new();
    }
    let mut content = String::new();
    file.read_to_string(&mut content).unwrap_or_default();
    content
}

/// Maximum byte length accepted from a single `prompt_confirm` read.
/// A real y/N answer is at most a few bytes; anything beyond this
/// limit is treated as "no" to prevent unbounded memory growth from
/// piped input.
pub const MAX_CONFIRM_BYTES: usize = 1_024;

/// Inner implementation of [`prompt_confirm`] that reads from an
/// arbitrary `BufRead`. Returns true only for trimmed,
/// case-insensitive `"y"` or `"yes"` responses that fit within
/// [`MAX_CONFIRM_BYTES`]. Oversized input is treated as "no".
///
/// Exposed so tests can drive the parsing logic without spawning a
/// pty or piping into stdin.
pub fn prompt_confirm_from(reader: &mut impl io::BufRead) -> bool {
    use io::{BufRead as _, Read as _};
    let mut input = String::new();
    let mut limited = reader.by_ref().take(MAX_CONFIRM_BYTES as u64 + 1);
    match limited.read_line(&mut input) {
        Err(_) => return false,
        Ok(0) => return false,
        Ok(_) => {}
    }
    if input.len() > MAX_CONFIRM_BYTES {
        return false;
    }
    matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// Print `msg` to stderr (with a `[y/N] ` suffix) and read a single
/// line from stdin. Returns true on `y`/`yes`, false otherwise.
///
/// stderr is used for the prompt (not stdout) so callers can pipe
/// stdout without interleaving prompt text into their pipe.
pub fn prompt_confirm(msg: &str) -> bool {
    use std::io::Write as _;
    eprint!("{msg} [y/N] ");
    let _ = io::stderr().flush();
    let stdin = io::stdin();
    let mut reader = io::BufReader::new(stdin.lock());
    prompt_confirm_from(&mut reader)
}
