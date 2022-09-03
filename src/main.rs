//! Tool to be run during startup to update nVidia binary drivers only when the resulting ABI
//! breakage can be immediately resolved by a kernel module reload or system restart, keeping the
//! packages from updating at other times.
//!
//! **Dependencies:** The following commands, at the paths specified in the following constants:
//!
//! - `apt-get`: [`APT_GET_PATH`]
//! - `apt-mark`: [`APT_MARK_PATH`]
//! - `dpkg-query`: [`DPKG_QUERY_PATH`]
//! - `modprobe`: [`MODPROBE_PATH`] (or `reboot` at [`REBOOT_PATH`])
//! - `rmmod`: [`RMMOD_PATH`] (or `reboot` at [`REBOOT_PATH`])

use std::collections::BTreeMap; // So user-visible output is sorted
use std::error::Error;
use std::process::Command;

/// Path to use for invoking the `apt-get` Command
///
/// (Hard-coded to an absolute path for security-reasons)
const APT_GET_PATH: &str = "/usr/bin/apt-get";

/// Path to use for invoking the `apt-mark` Command
///
/// (Hard-coded to an absolute path for security-reasons)
const APT_MARK_PATH: &str = "/usr/bin/apt-mark";

/// Path to use for invoking the `dpkg-query` Command
///
/// (Hard-coded to an absolute path for security-reasons)
const DPKG_QUERY_PATH: &str = "/usr/bin/dpkg-query";

/// Path to use for invoking the `reboot` Command
///
/// (Hard-coded to an absolute path for security-reasons)
const REBOOT_PATH: &str = "/sbin/reboot";

/// Path to use for invoking the `rmmod` Command
///
/// (Hard-coded to an absolute path for security-reasons)
const RMMOD_PATH: &str = "/sbin/rmmod";

/// Path to use for invoking the `modprobe` Command
///
/// (Hard-coded to an absolute path for security-reasons)
const MODPROBE_PATH: &str = "/sbin/modprobe";

/// Single definition of the kernel module name to load and unload
const NVIDIA_KMOD_NAME: &str = "nvidia";

/// Workaround for `ExitStatusError` being unstable
#[derive(Debug)]
struct CalledProcessError {
    /// The subprocess's exit code (or `None` if killed by a POSIX signal)
    pub code: Option<i32>,
}

impl std::fmt::Display for CalledProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Process returned non-success exit code {:?}", self.code)
    }
}
impl Error for CalledProcessError {}

/// Helper to deduplicate the boilerplate of handling errors with `Command`
///
/// Named after the Python `subprocess` function it mimics.
#[rustfmt::skip] // rustfmt bug causes inside of closure to migrate right on every save
macro_rules! check_call {
    ($cmd:expr) => {
        (|| {
            let status = $cmd.status()?;
            if !status.success() {
                // TODO: Nicer output
                return Err(CalledProcessError { code: status.code() }.into());
            }
            Ok::<std::process::ExitStatus, Box<dyn Error>>(status)
        })()
    };
}

/// An RAII-based mechanism for temporarily `apt-mark unhold`-ing packages
struct UnholdGuard {
    /// Names of packages to re-`apt-mark hold` on drop
    names: Vec<String>,
}

impl UnholdGuard {
    /// Construct a new guard and immediately un-hold the given packages
    pub fn new(names: Vec<String>) -> Result<Self, Box<dyn Error>> {
        eprintln!("Un-holding: {}", names.join(" "));
        check_call!(Command::new(APT_MARK_PATH).arg("unhold").arg("-qq").args(&names))?;
        Ok(Self { names })
    }
    /// Add more entries to the list of things to hold when the guard drops
    pub fn extend(&mut self, names: impl IntoIterator<Item = String>) {
        self.names.extend(names);
    }
}

impl Drop for UnholdGuard {
    fn drop(&mut self) {
        eprintln!("Re-holding: {}", self.names.join(" "));
        if !Command::new(APT_MARK_PATH)
            .arg("hold")
            .arg("-qq")
            .args(&self.names)
            .status()
            .expect("run apt-mark again to re-hold packages")
            .success()
        {
            panic!("Failed to re-mark packages as held: {}", self.names.join(" "));
        }
    }
}

/// Retrieve a map from installed packages with `nvidia` in the name to their version strings
fn get_nvidia_packages() -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let cmd_result = Command::new(DPKG_QUERY_PATH).arg("--list").arg("*nvidia*").output()?;

    if !cmd_result.status.success() {
        return Err(CalledProcessError { code: cmd_result.status.code() }.into());
    }

    let mut results = BTreeMap::new();
    for line in String::from_utf8(cmd_result.stdout)?.split('\n') {
        let mut fields = line.split_whitespace();
        if !matches!(fields.next(), Some("ii" | "hi")) {
            continue;
        }
        if let (Some(pkgname), Some(pkgver)) = (fields.next(), fields.next()) {
            results.insert(pkgname.to_owned(), pkgver.to_owned());
        }
    }
    Ok(results)
}

/// Un-pin nVidia packages, update them, and re-pin them
///
/// If `mark_only` is `true`, then don't actually update anything and just refresh the package pins
///
/// The return value indicates whether something was updated and a kernel module reload may be
/// necessary.
fn do_update(mark_only: bool) -> Result<bool, Box<dyn Error>> {
    if !mark_only {
        eprintln!("Updating package list...");
        check_call!(Command::new(APT_GET_PATH).arg("update"))?;
    }

    eprintln!("Getting list of eligible packages");
    let old_versions = get_nvidia_packages()?;

    let mut unhold_guard = UnholdGuard::new(old_versions.keys().cloned().collect())?;
    if mark_only {
        // Just go straight to dropping the guard
        return Ok(false);
    }

    eprintln!("Updating all packages list...");
    check_call!(Command::new(APT_GET_PATH).arg("dist-upgrade").arg("-y"))?;

    // Update the list of packages to re-hold and report whether a kernel module reload is needed
    eprintln!("Getting updated list of eligible packages");
    let new_versions = get_nvidia_packages()?;
    unhold_guard.extend(new_versions.keys().cloned());
    Ok(old_versions != new_versions)
}

/// Attempt to reload the nVidia kernel module. May trigger a reboot.
fn reload_nvidia() -> Result<(), Box<dyn Error>> {
    eprintln!("Attempting nvidia kernel module reload...");
    match check_call!(Command::new(RMMOD_PATH).arg(NVIDIA_KMOD_NAME)) {
        Ok(_) => {
            check_call!(Command::new(MODPROBE_PATH).arg(NVIDIA_KMOD_NAME))?;
        },
        Err(_) => {
            eprintln!("Module reload failed. Triggering reboot...");
            check_call!(Command::new(REBOOT_PATH))?;
        },
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut mark_only = false;

    // Basic CLI argument parser that doesn't rely on external crates
    let mut args = std::env::args();
    let cmd = args.next().expect("get argv[0] from std::env::args");
    #[allow(clippy::wildcard_in_or_patterns)] // Make "Help or unrecognized" intent clear
    for arg in args {
        match arg.as_ref() {
            "--mark-only" => {
                mark_only = true;
            },
            "-h" | "--help" | _ => {
                println!("Usage: {} [-h|--help|--mark-only]\n", cmd);
                println!("    -h | --help\t\tShow this message");
                println!(
                    "    --mark-only\t\tDon't actually update packages. Just re-hold packages."
                );
                println!("\nRequired external dependencies:\n");
                println!("    - {}", APT_GET_PATH);
                println!("    - {}", DPKG_QUERY_PATH);
                println!("    - {} (or {})", MODPROBE_PATH, REBOOT_PATH);
                println!("    - {} (or {})", RMMOD_PATH, REBOOT_PATH);
                return Ok(());
            },
        }
    }

    if do_update(mark_only)? {
        reload_nvidia()?;
    }
    Ok(())
}
