//! System clipboard text reader.
//!
//! Used by `cmd::paste_clipboard` to feed the nu Ctrl+V binding so a
//! paste bypasses the abbreviation trigger entirely (the binding does
//! not see individual space keystrokes during paste, so the
//! mid-paste-trigger problem documented for nu does not happen).
//!
//! ## Provider chain
//!
//! - Windows: native API via [`windows-sys`] — OpenClipboard +
//!   GetClipboardData(CF_UNICODETEXT) + GlobalLock, all wrapped in an
//!   RAII guard so panics still close the clipboard.
//! - WSL: tries Linux providers first (a Wayland or X server may be
//!   running under WSLg) and falls back to `powershell.exe Get-Clipboard`
//!   so users without a GUI session can still paste from Windows.
//! - Linux: `wl-paste` → `xclip -selection clipboard -o` →
//!   `xsel --clipboard --output`. First successful one wins.
//! - macOS: `pbpaste` (always available on stock macOS).
//!
//! Each external command is given a 500 ms cap (poll `try_wait` 50× at
//! 10 ms intervals) so a hung `xclip` against a missing X server
//! doesn't block the runex hook indefinitely.
//!
//! Empty clipboard returns `Ok(String::new())` rather than an error
//! variant — the cmd-side handler emits empty stdout, the nu binding
//! sees `is-empty`, no-op insert, no surprise.

use std::io;

#[cfg(unix)]
use std::sync::OnceLock;

/// Maximum bytes we are willing to inject from the clipboard into the
/// shell command line. Distinct from `MAX_HOOK_LINE_BYTES` (16 KiB)
/// because paste is a user-initiated single event whereas the hook
/// fires per keystroke; sharing the cap would conflate threat models.
/// 1 MiB is generous enough for multi-line code paste while still
/// putting an upper bound on the worst-case `commandline edit
/// --insert` string the shell has to absorb.
pub(crate) const MAX_CLIPBOARD_BYTES: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ClipboardError {
    #[error(
        "clipboard provider not available (install xclip / wl-paste / xsel, \
         or use WSL with powershell.exe in PATH)"
    )]
    NoProvider,
    // Only the unix external-provider path (xclip / wl-paste /
    // powershell.exe bridge) enforces the 500 ms deadline; the
    // Windows native reader is synchronous.
    #[cfg(unix)]
    #[error("clipboard read timed out after 500 ms")]
    Timeout,
    #[error("clipboard text exceeds maximum size of {cap} bytes ({actual} bytes)")]
    TooLarge { actual: usize, cap: usize },
    #[error("clipboard text is not valid UTF-8 / UTF-16")]
    Decode,
    #[error("OS error: {0}")]
    Io(#[from] io::Error),
}

/// Read the system clipboard text.
///
/// Returns `Ok(String::new())` when the clipboard is empty or holds
/// only non-text data (image, file list, …). Returns
/// [`ClipboardError::NoProvider`] when no usable provider can be
/// reached so the cmd-side handler can surface a helpful "install
/// xclip" hint.
pub(crate) fn read_clipboard_text() -> Result<String, ClipboardError> {
    let raw = read_raw()?;
    if raw.len() > MAX_CLIPBOARD_BYTES {
        return Err(ClipboardError::TooLarge {
            actual: raw.len(),
            cap: MAX_CLIPBOARD_BYTES,
        });
    }
    Ok(raw)
}

// ─── Windows native ─────────────────────────────────────────────────

#[cfg(windows)]
fn read_raw() -> Result<String, ClipboardError> {
    
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, OpenClipboard,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};

    /// `CF_UNICODETEXT = 13` per `winuser.h`. windows-sys exposes it
    /// via the constant; we redeclare here to avoid pulling in
    /// `Win32_UI_*` features for a single integer.
    const CF_UNICODETEXT: u32 = 13;

    /// RAII guard so `CloseClipboard` runs even on panic (e.g. the
    /// `String::from_utf16` path below). Pairing OpenClipboard with
    /// CloseClipboard is required by Windows; failing to close locks
    /// the clipboard for every other process on the system.
    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            // Safety: OpenClipboard succeeded (we only construct
            // this guard after a successful open). CloseClipboard
            // ignores subsequent calls and never panics.
            unsafe { CloseClipboard() };
        }
    }

    // Safety: passing `0` as HWND is the documented "use the current
    // task's foreground window" path. Returns 0 on failure (clipboard
    // is owned by another process — typical when the user is
    // mid-drag).
    let opened = unsafe { OpenClipboard(0 as _) };
    if opened == 0 {
        return Err(ClipboardError::NoProvider);
    }
    let _guard = ClipboardGuard;

    // Safety: GetClipboardData returns NULL when the requested format
    // is not available. CF_UNICODETEXT covers every text-bearing app
    // because Windows synthesizes it from CF_TEXT/CF_OEMTEXT
    // automatically.
    let handle: HANDLE = unsafe { GetClipboardData(CF_UNICODETEXT) };
    if handle.is_null() {
        // No text on clipboard (image / file list / etc.). Return
        // empty rather than an error — matches the contract that the
        // nu binding silently no-ops on empty.
        return Ok(String::new());
    }

    // Safety: GlobalLock returns NULL on failure. The returned ptr is
    // owned by the clipboard; we must call GlobalUnlock when done but
    // must NOT free it.
    let locked = unsafe { GlobalLock(handle as _) } as *const u16;
    if locked.is_null() {
        return Err(ClipboardError::Io(io::Error::other(
            "GlobalLock failed",
        )));
    }

    // Walk the UTF-16 string until we hit the NUL terminator. Cap the
    // walk at MAX_CLIPBOARD_BYTES / 2 (each u16 is 2 bytes) so a
    // malformed clipboard payload missing its terminator can't run us
    // off the end.
    let mut len = 0usize;
    let cap_u16 = MAX_CLIPBOARD_BYTES / 2;
    // Safety: clipboard data is guaranteed NUL-terminated when
    // CF_UNICODETEXT format is honoured. The cap guards malformed
    // payloads that omit the terminator.
    while len < cap_u16 && unsafe { *locked.add(len) } != 0 {
        len += 1;
    }

    // Safety: locked is a valid pointer to `len` u16 values
    // (established by the walk above). We hold the clipboard guard so
    // the OS won't reuse the buffer.
    let slice = unsafe { std::slice::from_raw_parts(locked, len) };
    let result = String::from_utf16(slice).map_err(|_| ClipboardError::Decode);

    // Safety: GlobalUnlock balances the GlobalLock above. Ignoring
    // the return value is documented behaviour for read-only locks.
    unsafe { GlobalUnlock(handle as _) };

    result
}

// ─── Unix (Linux / macOS / WSL) ─────────────────────────────────────

#[cfg(unix)]
fn read_raw() -> Result<String, ClipboardError> {
    let mut last_err: Option<ClipboardError> = None;
    for provider in providers() {
        match try_provider(provider) {
            Ok(text) => return Ok(strip_bom(text)),
            Err(ClipboardError::NoProvider) => continue, // command not found
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or(ClipboardError::NoProvider))
}

/// `(command, args)` for each clipboard provider, in priority order.
#[cfg(unix)]
fn providers() -> Vec<(&'static str, Vec<&'static str>)> {
    let mut chain: Vec<(&'static str, Vec<&'static str>)> = Vec::new();

    if cfg!(target_os = "macos") {
        chain.push(("pbpaste", vec![]));
        return chain;
    }

    // Linux (incl. WSL): try GUI providers first. xclip / xsel cope
    // when an X server is reachable; wl-paste covers Wayland.
    chain.push(("wl-paste", vec!["--no-newline"]));
    chain.push(("xclip", vec!["-selection", "clipboard", "-o"]));
    chain.push(("xsel", vec!["--clipboard", "--output"]));

    if is_wsl() {
        // WSLg may not be available on every WSL distro; fall back
        // to bridging into Windows via PowerShell. `Get-Clipboard`
        // returns the same CF_UNICODETEXT payload the native reader
        // would surface, but encoded UTF-16LE with a BOM (we strip).
        chain.push(("powershell.exe", vec!["-NoProfile", "-Command", "Get-Clipboard"]));
    }

    chain
}

#[cfg(unix)]
fn is_wsl() -> bool {
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|s| s.to_lowercase().contains("microsoft"))
            .unwrap_or(false)
    })
}

#[cfg(unix)]
fn try_provider(provider: (&str, Vec<&str>)) -> Result<String, ClipboardError> {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let (cmd, args) = provider;

    let mut child = match Command::new(cmd)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(ClipboardError::NoProvider);
        }
        Err(e) => return Err(ClipboardError::Io(e)),
    };

    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        match child.try_wait()? {
            Some(_status) => break,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ClipboardError::Timeout);
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        // wl-paste exits 1 when nothing is on the Wayland clipboard;
        // treat that like an empty clipboard rather than escalating.
        if cmd == "wl-paste" {
            return Ok(String::new());
        }
        return Err(ClipboardError::Io(io::Error::new(
            io::ErrorKind::Other,
            format!("{cmd} exited with status {}", output.status),
        )));
    }

    decode(&output.stdout)
}

#[cfg(unix)]
fn decode(bytes: &[u8]) -> Result<String, ClipboardError> {
    // PowerShell on WSL emits UTF-16LE with a leading BOM. Detect
    // that path and re-decode; everything else (xclip / wl-paste /
    // pbpaste) is already UTF-8.
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let payload = &bytes[2..];
        if payload.len() % 2 != 0 {
            return Err(ClipboardError::Decode);
        }
        let mut units = Vec::with_capacity(payload.len() / 2);
        for chunk in payload.chunks_exact(2) {
            units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        return String::from_utf16(&units).map_err(|_| ClipboardError::Decode);
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| ClipboardError::Decode)
}

#[cfg(unix)]
fn strip_bom(mut s: String) -> String {
    // UTF-8 BOM (EF BB BF) sometimes survives PowerShell pipeline →
    // decode; Get-Clipboard via -Raw also adds a trailing CRLF that
    // we leave alone (multi-line paste should preserve user intent).
    if s.starts_with('\u{FEFF}') {
        s.replace_range(..'\u{FEFF}'.len_utf8(), "");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_bytes_is_one_mib() {
        assert_eq!(MAX_CLIPBOARD_BYTES, 1024 * 1024);
    }

    #[test]
    fn too_large_error_includes_actual_and_cap() {
        let e = ClipboardError::TooLarge {
            actual: 2 * 1024 * 1024,
            cap: MAX_CLIPBOARD_BYTES,
        };
        let msg = format!("{e}");
        assert!(msg.contains("2097152"));
        assert!(msg.contains("1048576"));
    }

    #[cfg(unix)]
    #[test]
    fn strip_bom_removes_leading_utf8_bom() {
        assert_eq!(strip_bom("\u{FEFF}hello".to_string()), "hello");
    }

    #[cfg(unix)]
    #[test]
    fn strip_bom_preserves_text_without_bom() {
        assert_eq!(strip_bom("hello".to_string()), "hello");
    }

    #[cfg(unix)]
    #[test]
    fn decode_handles_utf8() {
        assert_eq!(decode(b"hello").unwrap(), "hello");
    }

    #[cfg(unix)]
    #[test]
    fn decode_handles_utf16le_bom() {
        // "hi" as UTF-16LE: 0x68 0x00 0x69 0x00
        let bytes = vec![0xFF, 0xFE, 0x68, 0x00, 0x69, 0x00];
        assert_eq!(decode(&bytes).unwrap(), "hi");
    }

    #[cfg(unix)]
    #[test]
    fn decode_rejects_truncated_utf16le_bom() {
        // BOM + 3 bytes (odd length).
        let bytes = vec![0xFF, 0xFE, 0x68, 0x00, 0x69];
        assert!(matches!(decode(&bytes), Err(ClipboardError::Decode)));
    }
}
