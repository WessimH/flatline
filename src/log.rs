//! A tiny append-only diagnostic log: `~/Library/Logs/Flatline/flatline.log`.
//!
//! Engine starts, errors, status changes and restart reasons are written
//! here so intermittent problems (a blocked tap after sleep, a device that
//! refuses to start) can be understood after the fact. The file is rotated
//! to `flatline.log.1` beyond 1 MB.

use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 1 << 20;

/// Path of the log file (`None` without a home directory).
pub fn path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join("Library/Logs/Flatline/flatline.log"))
}

/// `YYYY-MM-DD hh:mm:ss` in UTC.
fn timestamp() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", rem / 3600, rem / 60 % 60, rem % 60)
}

/// Appends one line to the log (and mirrors it to stderr). Never fails.
pub fn log(msg: impl AsRef<str>) {
    #[cfg(test)]
    {
        // Tests read the lines back instead of writing the user's log file.
        CAPTURED.with(|c| c.borrow_mut().push(msg.as_ref().to_owned()));
        return;
    }
    #[allow(unreachable_code)]
    let line = format!("{} UTC  {}\n", timestamp(), msg.as_ref());
    eprint!("{line}");
    let Some(p) = path() else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&p).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(&p, p.with_extension("log.1"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = f.write_all(line.as_bytes());
    }
}

#[cfg(test)]
thread_local! {
    static CAPTURED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Tests only: the lines logged on this thread since the last call.
#[cfg(test)]
pub fn take_captured() -> Vec<String> {
    CAPTURED.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn timestamp_has_the_expected_shape() {
        let t = super::timestamp();
        assert_eq!(t.len(), 19, "{t}");
        assert!(t.starts_with("20"), "{t}");
    }
}
