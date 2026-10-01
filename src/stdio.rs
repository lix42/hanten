//! Writes to the process's stdout and stderr that survive a reader going away.
//!
//! `println!`/`eprintln!` panic when a write fails, and Rust ignores `SIGPIPE`, so
//! `hanten … | head` (or `2>&1 | head`) turned a finished run into exit 101 and a
//! backtrace. Nothing here restores `SIGPIPE` instead: commands keep working after
//! the report (`--out` recipes, `roll`'s failure gate, `--strict`, telemetry), and
//! the exit code is the run's, not its reader's.

use std::fmt;
use std::io::{self, Write};

/// Whether a stdout line reached a reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    Written,
    /// The reader closed the pipe; the text, or its rest, was dropped.
    ReaderGone,
}

/// Write `text` and a newline to stdout and flush. A closed pipe is
/// [`Delivery::ReaderGone`]; any other failure (a full disk behind `>`) is an error.
#[allow(clippy::disallowed_methods)] // the one stdout writer
pub fn stdout_line(text: &str) -> io::Result<Delivery> {
    write_line(&mut io::stdout().lock(), text)
}

/// Write a line to stderr, ignoring every failure: stderr is the channel a failure
/// would be reported on. Outside tests it never panics, so it is safe in the lcms2
/// C callback.
pub fn stderr_line(args: fmt::Arguments<'_>) {
    // libtest captures only the print macros, so tests keep their stderr attached.
    #[cfg(test)]
    eprintln!("{args}");
    #[cfg(not(test))]
    #[allow(clippy::disallowed_methods)] // the one stderr writer
    let _ = writeln!(io::stderr().lock(), "{args}");
}

/// [`stdout_line`]'s logic over any writer.
fn write_line(w: &mut impl Write, text: &str) -> io::Result<Delivery> {
    let sent = w
        .write_all(text.as_bytes())
        .and_then(|()| w.write_all(b"\n"))
        .and_then(|()| w.flush());
    match sent {
        Ok(()) => Ok(Delivery::Written),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(Delivery::ReaderGone),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Accepts `budget` bytes, then fails every write with `kind`; `flush_fails`
    /// makes the flush fail instead.
    struct Failing {
        budget: usize,
        kind: io::ErrorKind,
        flush_fails: bool,
    }

    impl Write for Failing {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.budget == 0 {
                return Err(self.kind.into());
            }
            let n = buf.len().min(self.budget);
            self.budget -= n;
            Ok(n)
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.flush_fails {
                Err(self.kind.into())
            } else {
                Ok(())
            }
        }
    }

    fn failing(budget: usize, kind: io::ErrorKind) -> Failing {
        Failing {
            budget,
            kind,
            flush_fails: false,
        }
    }

    #[test]
    fn a_line_is_written_with_its_newline() {
        let mut out = Vec::new();
        assert_eq!(write_line(&mut out, "{}").unwrap(), Delivery::Written);
        assert_eq!(out, b"{}\n");
    }

    #[test]
    fn a_closed_pipe_is_the_reader_gone_at_any_point() {
        // Before the text, mid-text, at the newline, and at the flush.
        for budget in [0, 3, 5] {
            let mut w = failing(budget, io::ErrorKind::BrokenPipe);
            assert_eq!(
                write_line(&mut w, "{\"a\"}").unwrap(),
                Delivery::ReaderGone,
                "budget {budget}"
            );
        }
        let mut w = Failing {
            flush_fails: true,
            ..failing(usize::MAX, io::ErrorKind::BrokenPipe)
        };
        assert_eq!(write_line(&mut w, "{}").unwrap(), Delivery::ReaderGone);
    }

    #[test]
    fn any_other_failure_is_an_error() {
        let mut w = failing(2, io::ErrorKind::StorageFull);
        assert_eq!(
            write_line(&mut w, "{\"a\"}").unwrap_err().kind(),
            io::ErrorKind::StorageFull
        );
        let mut w = Failing {
            flush_fails: true,
            ..failing(usize::MAX, io::ErrorKind::Other)
        };
        assert_eq!(
            write_line(&mut w, "{}").unwrap_err().kind(),
            io::ErrorKind::Other
        );
    }
}
