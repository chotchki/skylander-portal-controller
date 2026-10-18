//! Reading RPCS3's own log while it runs (`RPCS3.log` in the home's cache dir),
//! the evidence the live tests go by when nothing reaches a display: which
//! guest files the game opened, and when.

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

/// The guest path in a `sys_fs_open(path=“...”, ...)` line, which RPCS3 logs at
/// warning level, so a default config has it.
pub fn opened_path(line: &str) -> Option<&str> {
    let rest = line.split_once("sys_fs_open(path=\u{201c}")?.1;
    Some(rest.split_once('\u{201d}')?.0)
}

/// The lines RPCS3 has appended to its log since the last call.
#[derive(Debug)]
pub struct LogTail {
    path: PathBuf,
    offset: u64,
    partial: String,
}

impl LogTail {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            offset: 0,
            partial: String::new(),
        }
    }

    /// Whole lines only: a line still being written waits for the next call.
    pub fn lines(&mut self) -> Vec<String> {
        let Ok(mut file) = std::fs::File::open(&self.path) else {
            return Vec::new();
        };
        // RPCS3 rewrites its log at every start: a shorter file is a new one.
        if file.metadata().is_ok_and(|m| m.len() < self.offset) {
            self.offset = 0;
            self.partial.clear();
        }
        let mut bytes = Vec::new();
        if file.seek(SeekFrom::Start(self.offset)).is_err() || file.read_to_end(&mut bytes).is_err()
        {
            return Vec::new();
        }
        self.offset += bytes.len() as u64;
        self.partial.push_str(&String::from_utf8_lossy(&bytes));
        let Some(end) = self.partial.rfind('\n') else {
            return Vec::new();
        };
        let complete: Vec<String> = self.partial[..end].lines().map(str::to_owned).collect();
        self.partial.drain(..=end);
        complete
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opened_paths_come_out_of_rpcs3s_log_line() {
        let line = "\u{b7}W 0:00:54.632944 {PPU[0x1000025] Thread (load) [libfs: 0x00bf2090]} sys_fs: \
                    sys_fs_open(path=\u{201c}/dev_bdvd/PS3_GAME/USRDIR/level/a.arc\u{201d}, flags=00, fd=*0x1)";
        assert_eq!(
            opened_path(line),
            Some("/dev_bdvd/PS3_GAME/USRDIR/level/a.arc")
        );
        // The reply line names the file without the quotes; only the request counts.
        assert_eq!(
            opened_path("sys_fs: sys_fs_open(): fd=4, Regular file, '/dev_bdvd/x'"),
            None
        );
        assert_eq!(opened_path("sys_fs_open(path=\u{201c}/unterminated"), None);
    }

    #[test]
    fn the_log_tail_hands_over_whole_lines_once() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("RPCS3.log");
        let mut tail = LogTail::new(path.clone());
        assert!(tail.lines().is_empty(), "no file yet");
        std::fs::write(&path, "one\ntw").unwrap();
        assert_eq!(tail.lines(), ["one"]);
        std::fs::write(&path, "one\ntwo\nthree\n").unwrap();
        assert_eq!(tail.lines(), ["two", "three"]);
        assert!(tail.lines().is_empty());
        std::fs::write(&path, "new\n").unwrap();
        assert_eq!(tail.lines(), ["new"], "a rewritten log starts over");
    }
}
