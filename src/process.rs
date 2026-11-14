//! Processes on this machine, for sessions that live as long as a process.

use rustix::io::{self, Errno};
use rustix::process::{self, Pid};

/// Whether a process with this id is running.
///
/// Asks the kernel without sending a signal. A process of another user still counts:
/// signalling it is denied, which says that it exists.
pub fn is_running(pid: u32) -> bool {
    is_running_with(pid, process::test_kill_process)
}

/// `is_running` with the signal check passed in, so tests can choose its result.
fn is_running_with(pid: u32, test_kill: impl Fn(Pid) -> io::Result<()>) -> bool {
    let Some(pid) = i32::try_from(pid).ok().and_then(Pid::from_raw) else {
        return false;
    };
    match test_kill(pid) {
        Ok(()) => true,
        Err(err) => err == Errno::PERM,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_running() {
        assert!(is_running(std::process::id()));
    }

    #[test]
    fn a_process_that_may_be_signalled_is_running() {
        assert!(is_running_with(42, |_| Ok(())));
    }

    #[test]
    fn a_process_that_must_not_be_signalled_is_running() {
        assert!(is_running_with(42, |_| Err(Errno::PERM)));
    }

    #[test]
    fn a_process_that_does_not_exist_is_not_running() {
        assert!(!is_running_with(42, |_| Err(Errno::SRCH)));
    }

    #[test]
    fn ids_that_cannot_name_a_process_are_not_running() {
        let never_asked = |_| panic!("the kernel was asked about an invalid id");
        assert!(!is_running_with(0, never_asked));
        assert!(!is_running_with(u32::MAX, never_asked));
    }
}
