//! Port of `CommandLine.c` — htop's command-line entry and flag output.
//!
//! The `-V` / `-h` flag printers and the `parseArguments` getopt_long switch
//! are ported; the interactive run loop (`CommandLine_run`) is still driven
//! from `main.rs` rather than ported wholesale.
#![allow(non_snake_case)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;

// The platform `Process_fields[]` table + count, for `--sort-key=help` and the
// column lookup. Selected by target, mirroring htop's per-platform link.
#[cfg(target_os = "macos")]
use crate::ported::darwin::darwinprocess::{Process_fields, LAST_PROCESSFIELD};
#[cfg(target_os = "dragonfly")]
use crate::ported::dragonflybsd::dragonflybsdprocess::{Process_fields, LAST_PROCESSFIELD};
#[cfg(target_os = "freebsd")]
use crate::ported::freebsd::freebsdprocess::{Process_fields, LAST_PROCESSFIELD};
#[cfg(target_os = "linux")]
use crate::ported::linux::linuxprocess::{Process_fields, LAST_PROCESSFIELD};
#[cfg(target_os = "netbsd")]
use crate::ported::netbsd::netbsdprocess::{Process_fields, LAST_PROCESSFIELD};
#[cfg(target_os = "openbsd")]
use crate::ported::openbsd::openbsdprocess::{Process_fields, LAST_PROCESSFIELD};
#[cfg(any(target_os = "solaris", target_os = "illumos"))]
use crate::ported::solaris::solarisprocess::{Process_fields, LAST_PROCESSFIELD};
#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "solaris",
    target_os = "illumos",
    target_os = "dragonfly"
)))]
use crate::ported::unsupported::unsupportedprocess::{Process_fields, LAST_PROCESSFIELD};

// getopt's result globals. The `libc` crate declares `getopt_long` and `option`
// for the BSD/apple target but not the `optarg`/`optind` externs, so bind the
// real libSystem/glibc symbols directly (same ones htop's getopt_long fills).
extern "C" {
    static mut optarg: *mut c_char;
    static mut optind: c_int;
    // BSD/macOS getopt reset flag; glibc resets via `optind = 0` instead.
    #[cfg(target_os = "macos")]
    static mut optreset: c_int;
}

/// htop's `VERSION` — a build-time macro produced by configure. The
/// faithful Rust equivalent is the crate version from `Cargo.toml`.
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

/// htop's `COPYRIGHT` macro (`configure.ac:1833`).
pub(crate) const COPYRIGHT: &str = "(C) MenkeTechnologies 2026.";

/// Port of `printVersionFlag(const char* name)` from `CommandLine.c`.
/// C: `printf("%s " VERSION "\n", name)`.
pub fn printVersionFlag(name: &str) {
    println!("{name} {VERSION}");
}

/// Port of `printHelpFlag(const char* name)` from `CommandLine.c`. The
/// C emits the version line, copyright, license, then the option list.
/// `HAVE_GETMOUSE` gates the `-M` line; the mouse is always compiled
/// in here, so it is emitted unconditionally. `Platform_longOptionsUsage`
/// is a no-op until platform options are ported.
///
/// The htoprs binary itself does not call this — its `-h` handler renders
/// the styled help screen in [`crate::extensions::help`] instead. This
/// faithful port is retained as the spec that styled screen tracks.
pub fn printHelpFlag(name: &str) {
    print!(
        "{name} {VERSION}\n\
         {COPYRIGHT}\n\
         Released under the MIT License.\n\n\
         -C --no-color                   Use a monochrome color scheme\n\
         -d --delay=DELAY                Set the delay between updates, in tenths of seconds\n\
         -F --filter=FILTER              Show only the commands matching the given filter\n   \
            --no-function-bar            Hide the function bar\n\
         -h --help                       Print this help screen\n\
         -H --highlight-changes[=DELAY]  Highlight new and old processes\n\
         -M --no-mouse                   Disable the mouse\n   \
            --no-meters                  Hide meters\n\
         -n --max-iterations=NUMBER      Exit htop after NUMBER iterations/frame updates\n\
         -p --pid=PID[,PID,PID...]       Show only the given PIDs\n   \
            --readonly                   Disable all system and process changing features\n\
         -s --sort-key=COLUMN            Sort by COLUMN in list view (try --sort-key=help for a list)\n\
         -t --tree                       Show the tree view (can be combined with -s)\n\
         -u --user[=USERNAME]            Show only processes for a given user (or $USER)\n\
         -U --no-unicode                 Do not use unicode but plain ASCII\n\
         -V --version                    Print version info\n\
         \n\
         Press F1 inside {name} for online help.\n\
         See 'man {name}' for more information.\n"
    );
}

/// Port of `typedef enum { STATUS_OK, STATUS_ERROR_EXIT, STATUS_OK_EXIT }
/// CommandLineStatus` (`CommandLine.h:11`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandLineStatus {
    /// `STATUS_OK` — parsing succeeded; continue into the TUI.
    Ok,
    /// `STATUS_ERROR_EXIT` — a bad argument; the caller exits non-zero.
    ErrorExit,
    /// `STATUS_OK_EXIT` — a flag that prints and exits zero (`-h`/`-V`/
    /// `--sort-key=help`).
    OkExit,
}

/// Port of `struct CommandLineSettings` (`CommandLine.c:80`) — the parsed CLI
/// flags. Fields that need still-unported startup substrate to *apply* are
/// nonetheless parsed and validated here (matching htop's error exits), and
/// carried for when the TUI startup path wires them.
#[derive(Debug)]
pub struct CommandLineSettings {
    /// C `Hashtable* pidMatchList` — modeled as the parsed pid list.
    pub pidMatchList: Vec<u32>,
    /// C `char* commFilter`.
    pub commFilter: Option<String>,
    /// C `uid_t userId` — `None` is the C `(uid_t)-1` "all users".
    pub userId: Option<u32>,
    /// C `int sortKey` — a [`ProcessField`](crate::ported::process::ProcessField) id.
    pub sortKey: i32,
    /// C `int delay` — `-1` until set.
    pub delay: i32,
    /// C `int iterationsRemaining`.
    pub iterationsRemaining: i32,
    /// C `bool useColors`.
    pub useColors: bool,
    /// C `bool enableMouse`.
    pub enableMouse: bool,
    /// C `bool treeView`.
    pub treeView: bool,
    /// C `bool allowUnicode`.
    pub allowUnicode: bool,
    /// C `int stableTreeView` — `-1` until set.
    pub stableTreeView: i32,
    /// C `bool highlightChanges`.
    pub highlightChanges: bool,
    /// C `int highlightDelaySecs` — `-1` until set.
    pub highlightDelaySecs: i32,
    /// C `bool readonly`.
    pub readonly: bool,
    /// C `bool hideMeters`.
    pub hideMeters: bool,
    /// C `bool hideFunctionBar`.
    pub hideFunctionBar: bool,
}

impl Default for CommandLineSettings {
    /// The C designated initializer in `parseArguments` (`CommandLine.c:125`).
    fn default() -> Self {
        CommandLineSettings {
            pidMatchList: Vec::new(),
            commFilter: None,
            userId: None,
            sortKey: 0,
            delay: -1,
            iterationsRemaining: -1,
            useColors: true,
            enableMouse: true,
            treeView: false,
            allowUnicode: true,
            stableTreeView: -1,
            highlightChanges: false,
            highlightDelaySecs: -1,
            readonly: false,
            hideMeters: false,
            hideFunctionBar: false,
        }
    }
}

/// Port of `static CommandLineStatus parseArguments(int argc, char** argv,
/// CommandLineSettings* flags)` from `CommandLine.c:123`.
///
/// Drives the same `getopt_long` htop uses (via `libc`), with the identical
/// option string and long-option table, so parsing, error exits, and
/// `--sort-key=help` output match htop byte-for-byte. `argv` includes the
/// program name at index 0, as `getopt` expects. The `-h`/`-V` cases are
/// reachable but the htoprs binary short-circuits those in `main.rs` to the
/// branded help/version first.
pub fn parseArguments(program: &str, argv: &[String]) -> (CommandLineStatus, CommandLineSettings) {
    // Reset getopt's global scan position so this is reentrant (the binary
    // calls it once per fresh process; the unit tests call it repeatedly).
    unsafe {
        #[cfg(target_os = "macos")]
        {
            optreset = 1;
            optind = 1;
        }
        #[cfg(not(target_os = "macos"))]
        {
            optind = 0;
        }
    }

    let mut flags = CommandLineSettings::default();

    // NO_COLOR env support (https://no-color.org/) — CommandLine.c:147.
    if let Some(nc) = std::env::var_os("NO_COLOR") {
        if !nc.is_empty() {
            flags.useColors = false;
        }
    }

    // Marshal argv into a null-terminated C `char**` for getopt_long.
    let c_args: Vec<CString> = argv
        .iter()
        .map(|s| CString::new(s.as_str()).unwrap_or_default())
        .collect();
    let mut c_argv: Vec<*mut c_char> = c_args.iter().map(|s| s.as_ptr() as *mut c_char).collect();
    c_argv.push(ptr::null_mut());
    let argc = argv.len() as c_int;

    let optstring = CString::new("hVMCs:t::d:n:u::Up:F:H::").expect("static optstring");

    // long_opts (CommandLine.c:154). `names` keeps the CStrings alive for the
    // duration of the getopt_long calls below. `has_arg`: 0=no,1=required,2=opt.
    let spec: [(&str, c_int, c_int); 18] = [
        ("help", 0, b'h' as c_int),
        ("version", 0, b'V' as c_int),
        ("delay", 1, b'd' as c_int),
        ("max-iterations", 1, b'n' as c_int),
        ("sort-key", 1, b's' as c_int),
        ("user", 2, b'u' as c_int),
        ("no-color", 0, b'C' as c_int),
        ("no-colour", 0, b'C' as c_int),
        ("no-mouse", 0, b'M' as c_int),
        ("no-unicode", 0, b'U' as c_int),
        ("no-meters", 0, 129),
        ("tree", 2, b't' as c_int),
        ("pid", 1, b'p' as c_int),
        ("filter", 1, b'F' as c_int),
        ("no-functionbar", 0, 130),
        ("no-function-bar", 0, 130),
        ("highlight-changes", 2, b'H' as c_int),
        ("readonly", 0, 128),
    ];
    let names: Vec<CString> = spec
        .iter()
        .map(|(n, _, _)| CString::new(*n).unwrap())
        .collect();
    let mut long_opts: Vec<libc::option> = spec
        .iter()
        .enumerate()
        .map(|(i, &(_, has_arg, val))| libc::option {
            name: names[i].as_ptr(),
            has_arg,
            flag: ptr::null_mut(),
            val,
        })
        .collect();
    long_opts.push(libc::option {
        name: ptr::null(),
        has_arg: 0,
        flag: ptr::null_mut(),
        val: 0,
    });

    // Helpers as closures (free `fn`s in `src/ported` must be C ports; these
    // are marshalling glue, not ports). `optarg` reads the getopt global.
    let optarg_string = || -> Option<String> {
        let p = unsafe { optarg };
        if p.is_null() {
            None
        } else {
            Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
        }
    };
    // sscanf("%16d") — a leading optional-sign integer; trailing junk ignored.
    let scan_int = |s: &str| -> Option<i32> {
        let t = s.trim_start();
        let bytes = t.as_bytes();
        let mut end = 0;
        if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
            end += 1;
        }
        let digits_start = end;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end == digits_start {
            return None;
        }
        t[..end].parse::<i32>().ok()
    };
    // parseTreeStableMode (CommandLine.c:104): classic/soft/hard aliases.
    let tree_stable = |arg: &str| -> Option<i32> {
        match arg {
            "0" | "classic" | "legacy" | "jumpy" => Some(0),
            "1" | "soft" | "stable" => Some(1),
            "2" | "hard" | "STABLE" => Some(2),
            _ => None,
        }
    };
    // The next argv token, if present and not an option / empty — the manual
    // optional-argument grab htop does for -u/-t/-H (CommandLine.c:248 etc.).
    let peek_next = |flags_delay_optind: c_int| -> Option<&String> {
        let i = flags_delay_optind as usize;
        argv.get(i).filter(|a| !a.is_empty() && !a.starts_with('-'))
    };

    loop {
        let opt = unsafe {
            libc::getopt_long(
                argc,
                c_argv.as_ptr(),
                optstring.as_ptr(),
                long_opts.as_ptr(),
                &mut 0,
            )
        };
        if opt == -1 {
            break; // EOF
        }
        match opt {
            x if x == b'h' as c_int => {
                printHelpFlag(program);
                return (CommandLineStatus::OkExit, flags);
            }
            x if x == b'V' as c_int => {
                printVersionFlag(program);
                return (CommandLineStatus::OkExit, flags);
            }
            x if x == b's' as c_int => {
                let Some(arg) = optarg_string() else {
                    return (CommandLineStatus::ErrorExit, flags);
                };
                if arg == "help" {
                    for j in 1..LAST_PROCESSFIELD {
                        let name = Process_fields[j].name;
                        if !name.is_empty() {
                            let desc = Process_fields[j].description.unwrap_or("");
                            println!("{name:>19} {desc}");
                        }
                    }
                    return (CommandLineStatus::OkExit, flags);
                }
                flags.sortKey = 0;
                for j in 1..LAST_PROCESSFIELD {
                    let name = Process_fields[j].name;
                    if name.is_empty() {
                        continue;
                    }
                    if arg == name {
                        flags.sortKey = j as i32;
                        break;
                    }
                }
                if flags.sortKey == 0 {
                    eprintln!("Error: invalid column \"{arg}\".");
                    return (CommandLineStatus::ErrorExit, flags);
                }
            }
            x if x == b'd' as c_int => {
                let Some(arg) = optarg_string() else {
                    return (CommandLineStatus::ErrorExit, flags);
                };
                match scan_int(&arg) {
                    // C clamps to [1, 100] (CommandLine.c:223-226).
                    Some(d) => flags.delay = d.clamp(1, 100),
                    None => {
                        eprintln!("Error: invalid delay value \"{arg}\".");
                        return (CommandLineStatus::ErrorExit, flags);
                    }
                }
            }
            x if x == b'n' as c_int => {
                let Some(arg) = optarg_string() else {
                    return (CommandLineStatus::ErrorExit, flags);
                };
                match scan_int(&arg) {
                    Some(n) if n > 0 => flags.iterationsRemaining = n,
                    Some(_) => {
                        eprintln!("Error: maximum iteration count must be positive.");
                        return (CommandLineStatus::ErrorExit, flags);
                    }
                    None => {
                        eprintln!("Error: invalid maximum iteration count \"{arg}\".");
                        return (CommandLineStatus::ErrorExit, flags);
                    }
                }
            }
            x if x == b'u' as c_int => {
                let mut username = optarg_string();
                if username.is_none() {
                    if let Some(next) = peek_next(unsafe { optind }) {
                        username = Some(next.clone());
                        unsafe { optind += 1 };
                    }
                }
                match username {
                    None => flags.userId = Some(unsafe { libc::geteuid() }),
                    Some(u) => match scan_int(&u) {
                        Some(v) if v >= 0 && u.trim() == v.to_string() => {
                            flags.userId = Some(v as u32)
                        }
                        _ => {
                            // Resolve a user name to its uid (Action_setUserOnly).
                            let uid = CString::new(u.as_str()).ok().and_then(|c| {
                                let pw = unsafe { libc::getpwnam(c.as_ptr()) };
                                if pw.is_null() {
                                    None
                                } else {
                                    Some(unsafe { (*pw).pw_uid })
                                }
                            });
                            match uid {
                                Some(id) => flags.userId = Some(id),
                                None => {
                                    eprintln!("Error: invalid user \"{u}\".");
                                    return (CommandLineStatus::ErrorExit, flags);
                                }
                            }
                        }
                    },
                }
            }
            x if x == b'C' as c_int => flags.useColors = false,
            x if x == b'M' as c_int => flags.enableMouse = false,
            x if x == b'U' as c_int => flags.allowUnicode = false,
            129 => flags.hideMeters = true,
            x if x == b't' as c_int => {
                let mut arg = optarg_string();
                if arg.is_none() {
                    if let Some(next) = peek_next(unsafe { optind }) {
                        if tree_stable(next).is_some() {
                            arg = Some(next.clone());
                            unsafe { optind += 1 };
                        }
                    }
                }
                if let Some(a) = arg {
                    match tree_stable(&a) {
                        Some(m) => flags.stableTreeView = m,
                        None => {
                            eprintln!(
                                "Error: invalid tree mode \"{a}\" (expected: classic, soft, hard (or 0, 1, 2))."
                            );
                            return (CommandLineStatus::ErrorExit, flags);
                        }
                    }
                }
                flags.treeView = true;
            }
            x if x == b'p' as c_int => {
                let Some(arg) = optarg_string() else {
                    return (CommandLineStatus::ErrorExit, flags);
                };
                for pid in arg.split(',') {
                    flags
                        .pidMatchList
                        .push(scan_int(pid).unwrap_or(0).max(0) as u32);
                }
            }
            x if x == b'F' as c_int => {
                let Some(arg) = optarg_string() else {
                    return (CommandLineStatus::ErrorExit, flags);
                };
                if arg.is_empty() || arg.starts_with('|') {
                    eprintln!("Error: invalid filter value \"{arg}\".");
                    return (CommandLineStatus::ErrorExit, flags);
                }
                flags.commFilter = Some(arg);
            }
            130 => flags.hideFunctionBar = true,
            x if x == b'H' as c_int => {
                let mut delay = optarg_string();
                if delay.is_none() {
                    if let Some(next) = peek_next(unsafe { optind }) {
                        delay = Some(next.clone());
                        unsafe { optind += 1 };
                    }
                }
                if let Some(d) = delay {
                    match scan_int(&d) {
                        Some(mut secs) => {
                            if secs < 1 {
                                secs = 1;
                            }
                            flags.highlightDelaySecs = secs;
                        }
                        None => {
                            eprintln!("Error: invalid highlight delay value \"{d}\".");
                            return (CommandLineStatus::ErrorExit, flags);
                        }
                    }
                }
                flags.highlightChanges = true;
            }
            128 => flags.readonly = true,
            // '?' (unknown option) and anything unrecognized: getopt already
            // reported it on stderr; htop's `default` returns error-exit.
            _ => return (CommandLineStatus::ErrorExit, flags),
        }
    }

    // Reject leftover non-option ARGV elements (CommandLine.c:362).
    let mut i = unsafe { optind } as usize;
    if i < argv.len() {
        eprint!("Error: unsupported non-option ARGV-elements:");
        while i < argv.len() {
            eprint!(" {}", argv[i]);
            i += 1;
        }
        eprintln!();
        return (CommandLineStatus::ErrorExit, flags);
    }

    (CommandLineStatus::Ok, flags)
}

/// Port of `static void setCommFilter(State* state, char** commFilter)`
/// (`CommandLine.c:328`). Applies the startup `--filter` (comm filter) to the
/// active table's incremental filter: `IncSet_setFilter(inc, *commFilter)` then
/// `table->incFilter = IncSet_filter(inc)`, mirroring the live
/// `MainPanel_eventHandler` filter path. The C `free(*commFilter); *commFilter =
/// NULL` becomes setting the owned `Option<String>` to `None`. macOS-gated,
/// matching its only caller ([`CommandLine_run`]).
#[cfg(target_os = "macos")]
fn setCommFilter(state: *mut crate::ported::action::State, commFilter: &mut Option<String>) {
    use crate::ported::incset::{IncSet_filter, IncSet_setFilter};

    // Table* table = state->host->activeTable;
    // SAFETY: state/host live for the program lifetime; activeTable is the
    // non-null back-pointer (the MainPanel_eventHandler precedent).
    let table = unsafe {
        (*(*state).host)
            .activeTable
            .expect("setCommFilter: host->activeTable is NULL")
    };
    // IncSet* inc = state->mainPanel->inc;
    let inc = unsafe { &mut (*(*state).mainPanel).inc };

    // IncSet_setFilter(inc, *commFilter); — caller guarantees a filter is set.
    let f = commFilter
        .as_deref()
        .expect("setCommFilter: called with no filter");
    IncSet_setFilter(inc, f);

    // table->incFilter = IncSet_filter(inc);
    let filter = IncSet_filter(inc).map(|s| s.to_string());
    unsafe {
        (*table).incFilter = filter;
    }

    // free(*commFilter); *commFilter = NULL;
    *commFilter = None;
}

/// Port of `int CommandLine_run(int argc, char** argv)` from `CommandLine.c:339`
/// — htop's program entry proper. Parses the arguments ([`parseArguments`], the
/// ported `getopt_long` half), and on a runnable parse assembles the shared
/// runtime object graph (`Machine` / `ProcessTable` / `Settings` / `Header` /
/// `MainPanel` / `ScreenManager`) and drives the [`ScreenManager_run`] main loop,
/// returning the process exit code. This is the single startup path: the binary
/// (`src/main.rs`) and the faithful [`crate::ported::htop::main`] both delegate
/// here.
///
/// The C run graph is a web of shared pointers that live until `exit`, so the
/// long-lived objects are heap-allocated with [`Box::into_raw`] (leaked for the
/// program's lifetime, as the C heap allocations are) and wired with raw
/// pointers — the faithful analog of the C pointer graph. Only the `macos`
/// (darwin) platform is assembled today; other targets report the gap and exit
/// non-zero.
///
/// [`ScreenManager_run`]: crate::ported::screenmanager::ScreenManager_run
#[cfg(target_os = "macos")]
pub fn CommandLine_run(program: &str, argv: &[String]) -> i32 {
    use crate::ported::action::State;
    use crate::ported::crt::{CRT_done, CRT_init, ColorScheme};
    use crate::ported::darwin::darwinmachine::{DarwinMachine, Machine_new, Machine_scan};
    use crate::ported::darwin::darwinprocesstable::{DarwinProcessTable, ProcessTable_new};
    use crate::ported::darwin::platform::Platform_init;
    use crate::ported::dynamiccolumn::DynamicColumns_new;
    use crate::ported::hashtable::{Hashtable, Hashtable_new};
    use crate::ported::header::{Header, Header_new, Header_populateFromSettings};
    use crate::ported::machine::{
        Machine, Machine_populateTablesFromSettings, Machine_scanTables, Machine_setTablesPanel,
    };
    use crate::ported::mainpanel::{
        MainPanel, MainPanel_new, MainPanel_setState, MainPanel_updateLabels,
    };
    use crate::ported::panel::Panel;
    use crate::ported::screenmanager::{ScreenManager_add, ScreenManager_new, ScreenManager_run};
    use crate::ported::settings::{ScreenSettings_setSortKey, Settings_new};
    use crate::ported::table::Table;
    use crate::ported::userstable::{UsersTable, UsersTable_new};

    // C: getopt_long parse (STATUS_OK_EXIT → 0, STATUS_ERROR_EXIT → 1).
    let mut flags = match parseArguments(program, argv) {
        (CommandLineStatus::OkExit, _) => return 0,
        (CommandLineStatus::ErrorExit, _) => return 1,
        (CommandLineStatus::Ok, flags) => flags,
    };

    // htoprs infra (no C analog): restore the terminal on a panic so a stub hit
    // mid-render does not leave the tty in raw mode / the alternate screen, then
    // persist the panic (message + backtrace) to ~/.cache/htoprs/crash.log so it
    // survives the alternate-screen teardown that would otherwise erase a stderr
    // report, and point the user at the log on the now-restored terminal.
    {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            use crossterm::{cursor, execute, terminal};
            let _ = terminal::disable_raw_mode();
            let mut out = std::io::stdout();
            let _ = execute!(out, terminal::LeaveAlternateScreen, cursor::Show);
            if let Some(path) = crate::extensions::crashlog::log_panic(info) {
                eprintln!("htoprs: crash logged to {}", path.display());
            }
            default(info);
        }));
    }

    // Platform_init() — mach-tick / scheduler-tick calibration (darwin).
    Platform_init();

    // UsersTable_new(): the uid->name cache handed to Machine_new; leaked for the
    // program's lifetime (as in C, until exit).
    let users_table: *mut UsersTable = Box::into_raw(Box::new(UsersTable_new()));

    // Machine_new(usersTable, userId): userId (uid_t)-1 == "all users".
    let host_raw: *mut DarwinMachine =
        Box::into_raw(Machine_new(Some(users_table as usize), u32::MAX));
    // SAFETY: host_raw is a fresh leaked allocation; the base Machine is at
    // offset 0 (`super_`), matching the C `&this->super` upcast.
    let host_ptr: *mut Machine = unsafe { &mut (*host_raw).super_ };

    // ProcessTable_new(host, pidMatchList).
    let pt_raw: *mut DarwinProcessTable =
        Box::into_raw(ProcessTable_new(host_ptr as *const Machine, None));
    // SAFETY: DarwinProcessTable -> ProcessTable (super_) -> Table (super_).
    let pt_table: *mut Table = unsafe { &mut (*pt_raw).super_.super_ };

    // Dynamic tables: DynamicColumns_new is ported; dynamic meters/screens are
    // empty on stock darwin (no PCP), so an empty owner Hashtable is faithful.
    let dm: *mut Hashtable = Box::into_raw(Box::new(Hashtable_new(7, true)));
    let dc: *mut Hashtable = Box::into_raw(Box::new(DynamicColumns_new()));
    let ds: *mut Hashtable = Box::into_raw(Box::new(Hashtable_new(7, true)));

    // Settings_new(host, dm, dc, ds).
    // SAFETY: host_raw is live; the &Machine borrow ends with this call.
    let mut settings = Settings_new(unsafe { &(*host_raw).super_ }, Some(dm), Some(dc), Some(ds));

    // Apply the parsed CLI flags to the settings (C CommandLine.c:375-400).
    let ss_index = settings.ssIndex as usize;
    if flags.delay != -1 {
        settings.delay = flags.delay;
    }
    if flags.treeView {
        settings.screens[ss_index].treeView = true;
    }
    if flags.highlightChanges {
        settings.highlightChanges = true;
    }
    if flags.highlightDelaySecs != -1 {
        settings.highlightDelaySecs = flags.highlightDelaySecs;
    }
    if flags.sortKey > 0 {
        if !flags.treeView {
            settings.screens[ss_index].treeView = false;
        }
        ScreenSettings_setSortKey(&mut settings.screens[ss_index], flags.sortKey);
    }
    if flags.hideFunctionBar {
        settings.hideFunctionBar = 2;
    }
    // SAFETY: host_raw is live and unaliased here.
    unsafe {
        (*host_raw).super_.iterationsRemaining = flags.iterationsRemaining as i64;
    }

    // Locals captured before `settings` moves into the host below. `--no-color`
    // loads CRT in monochrome without persisting the override.
    let delay = settings.delay;
    let color_scheme = if !flags.useColors {
        ColorScheme::COLORSCHEME_MONOCHROME as i32
    } else {
        settings.colorScheme
    };
    let h_layout = settings.hLayout;
    let tree_view = settings.screens[ss_index].treeView;

    // Machine_populateTablesFromSettings moves `settings` into host->settings.
    // SAFETY: host_raw is live and unaliased for this &mut.
    Machine_populateTablesFromSettings(unsafe { &mut (*host_raw).super_ }, settings, pt_table);

    // Header_new(host, hLayout) + Header_populateFromSettings.
    let header_raw: *mut Header =
        Box::into_raw(Box::new(Header_new(host_ptr as *const Machine, h_layout)));
    // SAFETY: header_raw and host->settings are both live.
    Header_populateFromSettings(unsafe { &mut *header_raw }, unsafe {
        (*host_raw).super_.settings.as_ref().unwrap()
    });

    // CRT_init: enters raw mode + alternate screen (crossterm). Mouse capture
    // stays off; `--no-unicode` and `-n` (retain screen for batch runs) honored.
    CRT_init(
        delay,
        color_scheme,
        false,
        flags.allowUnicode,
        flags.iterationsRemaining != -1,
    );

    // MainPanel_new() — build the process panel, its bars, and key bindings.
    let mut panel_box: Box<MainPanel> = Box::new(MainPanel_new());
    let panel_ptr: *mut MainPanel = &mut *panel_box;
    // SAFETY: panel_ptr is live; its embedded Panel is at `super_`.
    Machine_setTablesPanel(unsafe { &mut (*host_raw).super_ }, unsafe {
        &mut (*panel_ptr).super_ as *mut Panel
    });
    MainPanel_updateLabels(&mut panel_box, tree_view, flags.commFilter.is_some());

    // State: the shared UI state the handlers and ScreenManager dereference.
    let state_raw: *mut State = Box::into_raw(Box::new(State {
        host: host_ptr,
        mainPanel: panel_ptr,
        header: header_raw,
        failedUpdate: None,
        pauseUpdate: false,
        hideSelection: false,
        hideMeters: false,
    }));
    MainPanel_setState(&mut panel_box, state_raw);
    // C: if (flags.commFilter) setCommFilter(&state, &(flags.commFilter));
    if flags.commFilter.is_some() {
        setCommFilter(state_raw, &mut flags.commFilter);
    }

    // ScreenManager_new(header, host, state); add the MainPanel (moves the box;
    // panel_ptr stays valid — a Box's pointee address is move-stable).
    let mut scr = ScreenManager_new(header_raw, host_ptr, state_raw);
    ScreenManager_add(&mut scr, panel_box, -1);

    // Initial data collection.
    // SAFETY: host_raw is live and unaliased for these &mut calls.
    Machine_scan(unsafe { &mut *host_raw });
    Machine_scanTables(unsafe { &mut (*host_raw).super_ });

    // htoprs extensions: load the saved theme + bar style before the first frame
    // (the same extension hooks `ScreenManager_run` already weaves into the loop).
    crate::extensions::overlay::init_from_prefs();
    crate::extensions::barstyle::init_from_prefs();

    // The main loop.
    ScreenManager_run(&mut scr, None, None, None);

    // htoprs infra: the run loop unwound cleanly (not via a signal handler,
    // which logs itself and `_exit`s). Persist why — quit key, stdin EOF, or the
    // iteration limit — to crash.log so a normal exit is as traceable as a crash.
    crate::extensions::crashlog::flush_exit();

    CRT_done();
    0
}

/// Non-darwin [`CommandLine_run`]: the interactive TUI is wired for macOS in this
/// build. Validates the arguments (so `-h`/`-V`/bad flags still behave), then
/// reports the platform gap and returns a non-zero exit code.
#[cfg(not(target_os = "macos"))]
pub fn CommandLine_run(program: &str, argv: &[String]) -> i32 {
    match parseArguments(program, argv) {
        (CommandLineStatus::OkExit, _) => return 0,
        (CommandLineStatus::ErrorExit, _) => return 1,
        (CommandLineStatus::Ok, _) => {}
    }
    eprintln!("htoprs: the interactive TUI is wired for macOS (darwin) in this build");
    eprintln!("htoprs: run 'htoprs --help' for the command-line options");
    1
}

#[cfg(test)]
mod tests {
    // printVersionFlag / printHelpFlag write to stdout; their content
    // is pinned by the release smoke test (`htoprs --version`) and the
    // man page. The constants below guard against accidental edits.
    use super::*;

    #[test]
    fn version_is_crate_version() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
        assert!(!VERSION.is_empty());
    }

    /// getopt's scan state is process-global and not thread-safe; the real
    /// binary calls `parseArguments` once, but cargo runs these tests in
    /// parallel, so serialize the in-process calls behind this lock.
    static PARSE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Lock, then parse `opts` (program name prepended as argv[0]).
    fn parse(opts: &[&str]) -> (CommandLineStatus, CommandLineSettings) {
        let _g = PARSE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let argv: Vec<String> = std::iter::once("htoprs")
            .chain(opts.iter().copied())
            .map(String::from)
            .collect();
        parseArguments("htoprs", &argv)
    }

    #[test]
    fn no_args_is_ok() {
        let (status, flags) = parse(&[]);
        assert_eq!(status, CommandLineStatus::Ok);
        assert!(flags.useColors && flags.allowUnicode && flags.delay == -1);
    }

    #[test]
    fn unknown_flag_is_error_exit() {
        assert_eq!(
            parse(&["--definitely-not-a-flag"]).0,
            CommandLineStatus::ErrorExit
        );
    }

    #[test]
    fn leftover_non_option_is_error_exit() {
        assert_eq!(parse(&["stray"]).0, CommandLineStatus::ErrorExit);
    }

    #[test]
    fn valid_sort_key_resolves_to_field_id() {
        use crate::ported::process::ProcessField;
        let (status, flags) = parse(&["--sort-key=PERCENT_CPU"]);
        assert_eq!(status, CommandLineStatus::Ok);
        assert_eq!(flags.sortKey, ProcessField::PERCENT_CPU as i32);
    }

    #[test]
    fn invalid_sort_key_is_error_exit() {
        assert_eq!(parse(&["--sort-key=NOPE"]).0, CommandLineStatus::ErrorExit);
    }

    #[test]
    fn no_color_and_no_unicode_flags_apply() {
        let (status, flags) = parse(&["--no-color", "--no-unicode"]);
        assert_eq!(status, CommandLineStatus::Ok);
        assert!(!flags.useColors && !flags.allowUnicode);
    }

    #[test]
    fn delay_clamps_and_rejects_garbage() {
        let (ok, flags) = parse(&["-d", "999"]);
        assert_eq!(ok, CommandLineStatus::Ok);
        assert_eq!(flags.delay, 100); // clamped to the 1..=100 range
        assert_eq!(parse(&["-d", "abc"]).0, CommandLineStatus::ErrorExit);
    }
}
