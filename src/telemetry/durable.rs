//! Crash-safe, no-follow file primitives and cross-process locks for the upload
//! queue and the consent record (`docs/telemetry-strategy.md`).
//!
//! Every managed path is a regular file or directory owned by the current user:
//! a symlink, another file type, or (on Unix) another owner or a world-writable
//! mode is refused, so the caller fails closed. Group-writable is allowed: a
//! `umask 002` (user-private groups) makes every file so. Locks are `flock`-style
//! whole-file locks (`File::lock`) on files that are created once and never
//! unlinked, so a waiter can never end up on a different inode than the holder.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

/// `fsync` a directory, so a rename, create or unlink in it is durable. A no-op
/// where a directory cannot be opened as a file (Windows).
pub fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(dir)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

fn unsafe_entry(path: &Path, what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{} is {what}", path.display()),
    )
}

/// Refuse what another user could have planted or may rewrite.
fn check_owner(path: &Path, meta: &Metadata) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: `geteuid` has no preconditions and cannot fail.
        let me = unsafe { libc::geteuid() };
        if meta.uid() != me {
            return Err(unsafe_entry(path, "owned by another user"));
        }
        if meta.mode() & 0o002 != 0 {
            return Err(unsafe_entry(path, "writable by every user"));
        }
    }
    #[cfg(not(unix))]
    let _ = (path, meta);
    Ok(())
}

/// The metadata of a regular file at `path`, `None` when nothing is there.
/// A symlink, another file type or an unsafe owner is an error.
pub fn regular_or_missing(path: &Path) -> io::Result<Option<Metadata>> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
        Ok(meta) if meta.file_type().is_symlink() => Err(unsafe_entry(path, "a symlink")),
        Ok(meta) if !meta.is_file() => Err(unsafe_entry(path, "not a regular file")),
        Ok(meta) => check_owner(path, &meta).map(|()| Some(meta)),
    }
}

/// Whether a real directory is at `path` (`false` when nothing is). A symlink,
/// another file type or an unsafe owner is an error.
pub fn dir_or_missing(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
        Ok(meta) if meta.file_type().is_symlink() => Err(unsafe_entry(path, "a symlink")),
        Ok(meta) if !meta.is_dir() => Err(unsafe_entry(path, "not a directory")),
        Ok(meta) => check_owner(path, &meta).map(|()| true),
    }
}

/// Create the private directory `path` (mode 0700) if it is missing, durably.
pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    if dir_or_missing(path)? {
        return Ok(());
    }
    #[cfg(unix)]
    let created =
        std::os::unix::fs::DirBuilderExt::mode(&mut fs::DirBuilder::new(), 0o700).create(path);
    #[cfg(not(unix))]
    let created = fs::create_dir(path);
    match created {
        Ok(()) => {}
        // A concurrent creator won; check what it made.
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    if !dir_or_missing(path)? {
        return Err(unsafe_entry(path, "missing after creation"));
    }
    if let Some(parent) = path.parent() {
        sync_dir(parent)?;
    }
    Ok(())
}

fn no_follow(options: &mut OpenOptions) -> &mut OpenOptions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options
}

/// Create `path`, which must not exist (a symlink there counts as existing).
pub fn create_new(path: &Path) -> io::Result<File> {
    no_follow(OpenOptions::new().write(true).create_new(true)).open(path)
}

/// Open `path` for appending, creating it; never through a symlink.
pub fn open_append(path: &Path) -> io::Result<File> {
    regular_or_missing(path)?;
    no_follow(OpenOptions::new().append(true).create(true)).open(path)
}

/// Read a regular file of at most `limit` bytes, never through a symlink. A larger
/// one is `ErrorKind::FileTooLarge`, the only error that says so.
pub fn read_capped(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let file = no_follow(OpenOptions::new().read(true)).open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(unsafe_entry(path, "not a regular file"));
    }
    check_owner(path, &meta)?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            format!("{} is larger than expected", path.display()),
        ));
    }
    Ok(bytes)
}

/// Publish `bytes` at `dir/name` atomically: write a create-new `dir/tmp_name`,
/// `fsync` it, rename it over `name`, then `fsync` the directory.
pub fn publish(dir: &Path, tmp_name: &str, name: &str, bytes: &[u8]) -> io::Result<()> {
    let tmp = dir.join(tmp_name);
    let result = (|| {
        let mut file = create_new(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, dir.join(name))?;
        sync_dir(dir)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Remove `dir/name` if present and `fsync` the directory.
pub fn remove(dir: &Path, name: &str) -> io::Result<()> {
    match fs::remove_file(dir.join(name)) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(()) => sync_dir(dir),
    }
}

/// 32 random lowercase hex characters, for unique names and generations.
pub fn random_hex() -> io::Result<String> {
    super::EventId::random()
        .map(|id| id.hex())
        .ok_or_else(|| io::Error::other("the system random source failed"))
}

// ---------------------------------------------------------------------------
// Locks
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Shared,
    Exclusive,
}

/// A held lock, released when dropped (closing the file releases it).
#[derive(Debug)]
pub struct Held {
    _file: File,
}

/// How long to wait for a lock: `None` blocks.
pub type Wait = Option<Duration>;

/// Lock the file at `path` (created if missing, never truncated or removed).
/// `Ok(None)` when `wait` elapsed first.
pub fn lock(path: &Path, mode: Mode, wait: Wait) -> io::Result<Option<Held>> {
    regular_or_missing(path)?;
    let file = no_follow(OpenOptions::new().read(true).write(true).create(true)).open(path)?;
    let Some(wait) = wait else {
        match mode {
            Mode::Shared => file.lock_shared()?,
            Mode::Exclusive => file.lock()?,
        }
        return Ok(Some(Held { _file: file }));
    };
    let deadline = Instant::now() + wait;
    loop {
        let attempt = match mode {
            Mode::Shared => file.try_lock_shared(),
            Mode::Exclusive => file.try_lock(),
        };
        match attempt {
            Ok(()) => return Ok(Some(Held { _file: file })),
            Err(fs::TryLockError::WouldBlock) => {}
            Err(fs::TryLockError::Error(e)) => return Err(e),
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nc-durable-{tag}-{}-{}",
            std::process::id(),
            random_hex().unwrap()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn publish_replaces_atomically_and_leaves_no_temp() {
        let dir = scratch("publish");
        publish(&dir, ".a.tmp", "a", b"one").unwrap();
        publish(&dir, ".a.tmp", "a", b"two").unwrap();
        assert_eq!(fs::read(dir.join("a")).unwrap(), b"two");
        assert!(!dir.join(".a.tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_refused_everywhere() {
        let dir = scratch("symlink");
        fs::write(dir.join("real"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.join("real"), dir.join("link")).unwrap();
        assert!(regular_or_missing(&dir.join("link")).is_err());
        assert!(open_append(&dir.join("link")).is_err());
        assert!(read_capped(&dir.join("link"), 10).is_err());
        assert!(lock(&dir.join("link"), Mode::Shared, None).is_err());
        std::os::unix::fs::symlink(&dir, dir.join("dirlink")).unwrap();
        assert!(dir_or_missing(&dir.join("dirlink")).is_err());
        assert_eq!(fs::read(dir.join("real")).unwrap(), b"x");
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn group_writable_is_accepted_and_world_writable_is_not() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("modes");
        let file = dir.join("f");
        fs::write(&file, b"x").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o664)).unwrap();
        assert!(regular_or_missing(&file).is_ok(), "umask 002");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(regular_or_missing(&file).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_exclusive_lock_excludes_and_shared_locks_share() {
        let dir = scratch("lock");
        let path = dir.join("l.lock");
        let a = lock(&path, Mode::Shared, Some(Duration::ZERO)).unwrap();
        let b = lock(&path, Mode::Shared, Some(Duration::ZERO)).unwrap();
        assert!(a.is_some() && b.is_some());
        let x = lock(&path, Mode::Exclusive, Some(Duration::from_millis(20))).unwrap();
        assert!(x.is_none(), "an exclusive lock waits out shared holders");
        drop((a, b));
        let x = lock(&path, Mode::Exclusive, Some(Duration::ZERO)).unwrap();
        assert!(x.is_some());
        assert!(
            lock(&path, Mode::Shared, Some(Duration::ZERO))
                .unwrap()
                .is_none()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_capped_refuses_oversized_files() {
        let dir = scratch("cap");
        fs::write(dir.join("f"), b"12345").unwrap();
        assert_eq!(read_capped(&dir.join("f"), 5).unwrap(), b"12345");
        assert!(read_capped(&dir.join("f"), 4).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
