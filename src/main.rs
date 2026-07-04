//! htoprs entry point.
//!
//! `-V` / `-h` are handled directly via the ported `CommandLine.c` printers.
//! Any other invocation launches the interactive TUI by assembling the same
//! runtime object graph htop's `CommandLine_run` builds (`CommandLine.c:339`)
//! and driving the ported [`ScreenManager_run`] main loop.
//!
//! The C run graph is a web of shared pointers (`State` points at the
//! `Machine`/`MainPanel`/`Header`; the `ScreenManager`, `Header`, and
//! `ProcessTable` all hold a `Machine*`; etc.). Those objects live until the
//! process exits, so the long-lived ones are heap-allocated with
//! [`Box::into_raw`] (leaked for the program's lifetime, exactly as the C heap
//! allocations live until `exit`) and wired together with raw pointers — the
//! faithful analog of the C pointer graph.

use htoprs::ported::commandline;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let name = args
        .first()
        .and_then(|p| p.rsplit('/').next())
        .filter(|s| !s.is_empty())
        .unwrap_or("htoprs");

    // htoprs routes -h/--help and -V/--version to the branded help/version
    // screens (an intentional divergence from htop's plain printers), so
    // short-circuit those before the faithful getopt parse below.
    for arg in &args[1..] {
        match arg.as_str() {
            "-V" | "--version" => {
                commandline::printVersionFlag(name);
                return;
            }
            "-h" | "--help" => {
                htoprs::extensions::help::print_help(name);
                return;
            }
            _ => {}
        }
    }

    // Faithful CommandLine.c getopt_long parse for every other flag: validates
    // values, handles `--sort-key=help`, and rejects unknown options exactly as
    // htop does. STATUS_OK_EXIT → exit 0 (already printed), ERROR_EXIT → exit 1.
    let flags = match commandline::parseArguments(name, &args) {
        (commandline::CommandLineStatus::OkExit, _) => return,
        (commandline::CommandLineStatus::ErrorExit, _) => std::process::exit(1),
        (commandline::CommandLineStatus::Ok, flags) => flags,
    };

    run_tui(flags);
}

/// Restore the terminal (leave raw mode / alternate screen / show cursor) on a
/// panic so a stub hit mid-render does not leave the tty garbled.
#[cfg(target_os = "macos")]
fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        use crossterm::{cursor, execute, terminal};
        let _ = terminal::disable_raw_mode();
        let mut out = std::io::stdout();
        let _ = execute!(out, terminal::LeaveAlternateScreen, cursor::Show);
        default(info);
    }));
}

#[cfg(target_os = "macos")]
fn run_tui(flags: commandline::CommandLineSettings) {
    use htoprs::ported::action::State;
    use htoprs::ported::crt::{CRT_done, CRT_init, ColorScheme};
    use htoprs::ported::darwin::darwinmachine::{DarwinMachine, Machine_new, Machine_scan};
    use htoprs::ported::darwin::darwinprocesstable::{DarwinProcessTable, ProcessTable_new};
    use htoprs::ported::darwin::platform::Platform_init;
    use htoprs::ported::dynamiccolumn::DynamicColumns_new;
    use htoprs::ported::hashtable::{Hashtable, Hashtable_new};
    use htoprs::ported::header::{Header, Header_new, Header_populateFromSettings};
    use htoprs::ported::machine::{
        Machine, Machine_populateTablesFromSettings, Machine_scanTables, Machine_setTablesPanel,
    };
    use htoprs::ported::mainpanel::{
        MainPanel, MainPanel_new, MainPanel_setState, MainPanel_updateLabels,
    };
    use htoprs::ported::panel::Panel;
    use htoprs::ported::screenmanager::{ScreenManager_add, ScreenManager_new, ScreenManager_run};
    use htoprs::ported::settings::{ScreenSettings_setSortKey, Settings_new};
    use htoprs::ported::table::Table;

    install_panic_hook();

    // Platform_init() — mach-tick / scheduler-tick calibration (darwin).
    Platform_init();

    // UsersTable_new(): the uid->name cache htop's `CommandLine_run` builds and
    // hands to `Machine_new`. Leaked for the program's lifetime (as in C, where
    // it lives until exit) and stored on the machine as an opaque pointer; the
    // process scan populates it via `UsersTable_getRef`, and the `u` user-filter
    // picker iterates it. Without this the machine carried no users table, so
    // the `u` menu listed nobody.
    let users_table: *mut htoprs::ported::userstable::UsersTable =
        Box::into_raw(Box::new(htoprs::ported::userstable::UsersTable_new()));

    // Machine_new(usersTable, userId): userId (uid_t)-1 == "all users".
    let host_raw: *mut DarwinMachine =
        Box::into_raw(Machine_new(Some(users_table as usize), u32::MAX));
    // SAFETY: host_raw is a fresh leaked allocation; the embedded base Machine
    // is at offset 0 (`super_`), matching the C `&this->super` upcast.
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

    // Apply the parsed CLI flags to the settings, mirroring the flag block in
    // C `CommandLine_run` (`CommandLine.c:375-400`). `settings->ss` is the
    // active screen `settings.screens[settings.ssIndex]`. (Mouse capture stays
    // off — an intentional htoprs divergence — so `--no-mouse`/`enableMouse`
    // are not wired to `CRT_init` below.)
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
        // Do not implicitly enable tree view when '-s' is given.
        if !flags.treeView {
            settings.screens[ss_index].treeView = false;
        }
        ScreenSettings_setSortKey(&mut settings.screens[ss_index], flags.sortKey);
    }
    if flags.hideFunctionBar {
        settings.hideFunctionBar = 2;
    }
    // host->iterationsRemaining = flags.iterationsRemaining
    // SAFETY: host_raw is live and unaliased here.
    unsafe {
        (*host_raw).super_.iterationsRemaining = flags.iterationsRemaining as i64;
    }

    // Locals captured before `settings` is moved into the host below. `--no-color`
    // loads CRT in monochrome without persisting the override (C keeps the config
    // value for save, restoring it after `CRT_init`, CommandLine.c:373/407).
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
    // stays off; `--no-unicode` and `-n` (retain screen for batch runs) are honored.
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

    // ScreenManager_new(header, host, state); add the MainPanel (moves the box;
    // panel_ptr stays valid — a Box's pointee address is move-stable).
    let mut scr = ScreenManager_new(header_raw, host_ptr, state_raw);
    ScreenManager_add(&mut scr, panel_box, -1);

    // Initial data collection.
    // SAFETY: host_raw is live and unaliased for these &mut calls.
    Machine_scan(unsafe { &mut *host_raw });
    Machine_scanTables(unsafe { &mut (*host_raw).super_ });

    // htoprs extension: load the saved theme (if any) and apply its colors
    // before the first frame, so a previously-chosen theme is active on launch.
    htoprs::extensions::overlay::init_from_prefs();
    // htoprs extension: restore the saved bar fill style (b) the same way.
    htoprs::extensions::barstyle::init_from_prefs();

    // The main loop.
    ScreenManager_run(&mut scr, None, None, None);

    CRT_done();
}

#[cfg(not(target_os = "macos"))]
fn run_tui(_flags: commandline::CommandLineSettings) {
    eprintln!("htoprs: the interactive TUI is wired for macOS (darwin) in this build");
    eprintln!("htoprs: run 'htoprs --help' for the command-line options");
    std::process::exit(1);
}
