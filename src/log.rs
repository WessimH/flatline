//! A tiny append-only diagnostic log: `~/Library/Logs/Flatline/flatline.log`.
//!
//! Engine starts, errors, status changes and restart reasons are written
//! here so intermittent problems (a blocked tap after sleep, a device that
//! refuses to start) can be understood after the fact. The file is rotated
//! to `flatline.log.1` beyond 1 MB.

use std::io::Write;
use std::path::{Path, PathBuf};
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
    // Tests read the lines back instead of writing the user's log file.
    #[cfg(test)]
    CAPTURED.with(|c| c.borrow_mut().push(msg.as_ref().to_owned()));
    #[cfg(not(test))]
    {
        let line = format!("{} UTC  {}\n", timestamp(), msg.as_ref());
        eprint!("{line}");
        if let Some(p) = path() {
            append(&p, &line);
        }
    }
}

/// Appends `line` to the file at `p`, creating its folder, and first moves
/// the file to `<name>.1` when it is over [`MAX_BYTES`]. Never fails.
fn append(p: &Path, line: &str) {
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(p).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(p, p.with_extension("log.1"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
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
    use super::*;

    #[test]
    fn timestamp_has_the_expected_shape() {
        let t = timestamp();
        assert_eq!(t.len(), 19, "{t}");
        assert!(t.starts_with("20"), "{t}");
    }

    #[test]
    fn the_log_lives_in_the_user_library() {
        if std::env::var_os("HOME").is_some() {
            let p = path().expect("a home directory");
            assert!(p.ends_with("Library/Logs/Flatline/flatline.log"), "{}", p.display());
        }
    }

    /// `<temp>/flatline-log-<test>-<pid>/Logs/flatline.log` in a fresh
    /// folder: never the user's real log.
    fn temp_log(test: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("flatline-log-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root.join("Logs/flatline.log")
    }

    fn remove(p: &Path) {
        let _ = std::fs::remove_dir_all(p.parent().and_then(Path::parent).expect("temp root"));
    }

    #[test]
    fn lines_are_appended_and_the_folder_is_created() {
        let p = temp_log("append");
        append(&p, "one\n");
        append(&p, "two\n");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "one\ntwo\n");
        remove(&p);
    }

    #[test]
    fn a_full_log_moves_to_log_1() {
        let p = temp_log("rotate");
        // A single line longer than the limit is written whole.
        let long = "x".repeat(MAX_BYTES as usize + 10) + "\n";
        append(&p, &long);
        assert_eq!(std::fs::metadata(&p).unwrap().len(), long.len() as u64);
        // The next line starts a new file; the full one becomes flatline.log.1.
        append(&p, "after\n");
        let old = p.with_extension("log.1");
        assert!(old.ends_with("flatline.log.1"), "{}", old.display());
        assert_eq!(std::fs::metadata(&old).unwrap().len(), long.len() as u64);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "after\n");
        // A second rotation replaces the previous flatline.log.1.
        std::fs::write(&p, format!("{long}second\n")).unwrap();
        append(&p, "again\n");
        assert!(std::fs::read_to_string(&old).unwrap().ends_with("second\n"));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "again\n");
        remove(&p);
    }

    #[test]
    fn a_log_at_the_limit_is_kept() {
        let p = temp_log("limit");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "x".repeat(MAX_BYTES as usize)).unwrap();
        append(&p, "\n");
        assert!(!p.with_extension("log.1").exists());
        assert_eq!(std::fs::metadata(&p).unwrap().len(), MAX_BYTES + 1);
        remove(&p);
    }

    #[test]
    fn an_unwritable_log_is_ignored() {
        // The log's folder is a file: nothing is written, and nothing panics.
        let p = temp_log("unwritable");
        let dir = p.parent().unwrap();
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
        std::fs::write(dir, "not a folder").unwrap();
        append(&p, "lost\n");
        assert!(!p.exists());
        remove(&p);
    }

    #[test]
    fn tests_capture_the_lines_instead_of_writing() {
        take_captured();
        log("hello");
        log(String::from("world"));
        assert_eq!(take_captured(), ["hello", "world"]);
        assert!(take_captured().is_empty());
    }
}
