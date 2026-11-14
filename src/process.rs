//! Processes on this machine, for sessions that live as long as a process.

use rustix::io::Errno;
use rustix::process::{self, Pid};

/// Whether a process with this id is running.
///
/// Asks the kernel without sending a signal. A process of another user still counts:
/// signalling it is denied, which says that it exists.
pub fn is_running(pid: u32) -> bool {
    let Some(pid) = i32::try_from(pid).ok().and_then(Pid::from_raw) else {
        return false;
    };
    match process::test_kill_process(pid) {
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
    fn init_is_running_even_when_it_cannot_be_signalled() {
        assert!(is_running(1));
    }

    #[test]
    fn a_process_that_exited_is_not_running() {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert!(!is_running(pid));
    }

    #[test]
    fn ids_that_cannot_name_a_process_are_not_running() {
        assert!(!is_running(0));
        assert!(!is_running(u32::MAX));
    }
}
