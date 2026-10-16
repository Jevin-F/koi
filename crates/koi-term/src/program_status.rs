//! Program status reports (OSC 7501): a program tells the terminal whether
//! it is idle, working, blocked on the user, done, failed, or gone (`clear`).
//! The terminal state machine ignores these, so the output is scanned for
//! them separately.

/// A report: `ESC ] 7501 ; state=…:app=…:… ST`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramStatus {
    pub state: String,
    /// Name of the reporting program, such as `pi`.
    pub app: Option<String>,
}

const PREFIX: &[u8] = b"\x1b]7501;";
const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
/// Reports with a longer body are not valid and are dropped.
const MAX_BODY: usize = 4096;

/// Finds reports in output that arrives in arbitrary pieces.
#[derive(Default)]
pub struct Scanner {
    /// Bytes of [`PREFIX`] matched so far.
    matched: usize,
    /// Body of the report being read, once the prefix is complete.
    body: Option<Vec<u8>>,
    /// The body was followed by an `ESC`, which starts the `ESC \` terminator.
    escape: bool,
}

impl Scanner {
    /// Scans the next piece of output, calling `found` for each complete report.
    pub fn feed(&mut self, bytes: &[u8], mut found: impl FnMut(ProgramStatus)) {
        for &byte in bytes {
            let Some(mut body) = self.body.take() else {
                self.match_prefix(byte);
                continue;
            };
            if byte == BEL || (self.escape && byte == b'\\') {
                self.escape = false;
                if let Some(status) = parse(&body) {
                    found(status);
                }
            } else if self.escape {
                // Not a report after all; its `ESC` may begin the next one.
                self.escape = false;
                self.matched = 1;
                self.match_prefix(byte);
            } else if byte == ESC {
                self.escape = true;
                self.body = Some(body);
            } else if body.len() < MAX_BODY {
                body.push(byte);
                self.body = Some(body);
            }
        }
    }

    fn match_prefix(&mut self, byte: u8) {
        if byte == PREFIX[self.matched] {
            self.matched += 1;
            if self.matched == PREFIX.len() {
                self.matched = 0;
                self.body = Some(Vec::new());
            }
        } else {
            self.matched = usize::from(byte == ESC);
        }
    }
}

/// The `state` and `app` pairs of a report body; `None` for a body without
/// a state, such as a support query.
fn parse(body: &[u8]) -> Option<ProgramStatus> {
    let body = std::str::from_utf8(body).ok()?;
    let value = |key: &str| {
        body.split(':')
            .find_map(|pair| pair.strip_prefix(key)?.strip_prefix('='))
            .map(str::to_owned)
    };
    Some(ProgramStatus {
        state: value("state")?,
        app: value("app"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(pieces: &[&[u8]]) -> Vec<ProgramStatus> {
        let mut scanner = Scanner::default();
        let mut found = Vec::new();
        for piece in pieces {
            scanner.feed(piece, |status| found.push(status));
        }
        found
    }

    fn status(state: &str, app: Option<&str>) -> ProgramStatus {
        ProgramStatus {
            state: state.into(),
            app: app.map(Into::into),
        }
    }

    #[test]
    fn finds_reports_between_other_output() {
        let output = b"text\x1b]0;title\x07\x1b]7501;state=working:app=pi:msg=aGk=\x1b\\more\x1b]7501;state=done\x07";
        assert_eq!(
            scan(&[output]),
            [status("working", Some("pi")), status("done", None)]
        );
    }

    #[test]
    fn reports_may_be_split_anywhere() {
        let output = b"\x1b]7501;state=blocked:app=pi\x1b\\";
        for at in 1..output.len() {
            let (first, second) = output.split_at(at);
            assert_eq!(scan(&[first, second]), [status("blocked", Some("pi"))]);
        }
    }

    #[test]
    fn queries_and_broken_reports_are_skipped() {
        assert!(scan(&[b"\x1b]7501;?\x1b\\"]).is_empty());
        assert!(scan(&[b"\x1b]7501;state=working\x1b[0m"]).is_empty());
        // An `ESC` that ends a broken report may start a real one.
        assert_eq!(
            scan(&[b"\x1b]7501;junk\x1b\x1b]7501;state=clear\x07"]),
            [status("clear", None)]
        );
        let long = [b"\x1b]7501;state=".as_slice(), &[b'a'; MAX_BODY], b"\x07"].concat();
        assert!(scan(&[&long]).is_empty());
    }
}
