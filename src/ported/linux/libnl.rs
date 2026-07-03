//! `LibNl.c` — delay-accounting over generic netlink.
//!
//! The C file (`#ifndef HAVE_DELAYACCT #error`) reads per-process
//! delay-accounting (`struct taskstats`) from the kernel TASKSTATS generic
//! netlink family. Upstream htop `dlopen`s `libnl-3` / `libnl-genl-3` and
//! resolves a `sym_nl_*` / `sym_genl*` symbol table at runtime.
//!
//! This port speaks the netlink wire protocol directly through the pure-Rust
//! `neli` crate — **no FFI, no `dlopen`**. Because `neli` is statically
//! linked, the C `load_libnl` / `unload_libnl` dynamic-loading dance has no
//! runtime work left: symbol availability is a compile-time guarantee. Those
//! two functions are kept (same names, same call sites) as the degenerate
//! success/no-op forms of their C originals.
//!
//! `neli` also has no callback dispatcher, so htop's `nl_socket_modify_cb` +
//! `nl_recvmsgs_default` callback loop becomes a direct `send` / `recv`:
//! [`handleNetlinkMsg`] is invoked by [`LibNl_readDelayAcctData`] on the
//! received message rather than by a libnl callback. Its C return value
//! (`NL_OK` / `NL_SKIP`, consumed only by the libnl dispatcher) is therefore
//! dropped in favor of a unit return.
//!
//! `cfg(target_os = "linux")` carries the real implementation; other targets
//! mirror htop's `HAVE_DELAYACCT`-off variant (the file is absent there) with
//! no-op / no-data bodies so the shared build still compiles.
#![allow(non_snake_case)]
#![allow(dead_code)]

#[cfg(target_os = "linux")]
pub use linux_impl::*;

#[cfg(target_os = "linux")]
mod linux_impl {
    use crate::ported::linux::linuxprocess::LinuxProcess;
    use crate::ported::linux::linuxprocesstable::LinuxProcessTable;
    use crate::ported::process::Process_getPid;

    use neli::consts::nl::{NlmF, NlmFFlags};
    use neli::consts::socket::NlFamily;
    use neli::genl::{Genlmsghdr, Nlattr};
    use neli::nl::{NlPayload, Nlmsghdr};
    use neli::socket::NlSocketHandle;
    use neli::types::{Buffer, GenlBuffer};

    // Constants from <linux/taskstats.h> (uapi). Ported literally rather than
    // pulled through FFI so no kernel headers are required at build time.
    const TASKSTATS_GENL_NAME: &str = "TASKSTATS";
    const TASKSTATS_VERSION: u8 = 14;

    // enum taskstats_cmds
    const TASKSTATS_CMD_GET: u8 = 1;
    // enum taskstats_cmd_attrs
    const TASKSTATS_CMD_ATTR_PID: u16 = 1;
    // enum taskstats_type_attrs
    const TASKSTATS_TYPE_STATS: u16 = 3;
    const TASKSTATS_TYPE_AGGR_PID: u16 = 4;
    const TASKSTATS_TYPE_NULL: u16 = 6;

    /// Port of `LibNl.c:77` (`static int load_libnl(void)`).
    ///
    /// Upstream `dlopen`s `libnl-3.so` / `libnl-genl-3.so` and resolves the
    /// `sym_*` function-pointer table, returning `-1` on any failure. With
    /// `neli` statically linked there is nothing to load — the symbols are
    /// resolved at compile time — so this is the degenerate success path
    /// (`return 0`). Kept as a function (and called from
    /// [`initNetlinkSocket`]) to preserve the C control flow.
    pub fn load_libnl() -> i32 {
        0
    }

    /// Port of `LibNl.c:48` (`static void unload_libnl(void)`).
    ///
    /// Upstream NULLs the `sym_*` pointers and `dlclose`s the two libnl
    /// handles. With `neli` statically linked there are no handles and no
    /// function pointers to clear, so this is a no-op — the mirror of
    /// `load_libnl`'s no-load.
    pub fn unload_libnl() {}

    /// Port of `LibNl.c:134` (`static void initNetlinkSocket(LinuxProcessTable* this)`).
    ///
    /// Opens the generic-netlink socket and resolves the TASKSTATS family id,
    /// storing both on `this`. `neli`'s [`NlSocketHandle::connect`] fuses the C
    /// `nl_socket_alloc` + `nl_connect(NETLINK_GENERIC)`; `resolve_genl_family`
    /// replaces `genl_ctrl_resolve`.
    pub fn initNetlinkSocket(this: &mut LinuxProcessTable) {
        if load_libnl() < 0 {
            return;
        }

        let sock = match NlSocketHandle::connect(NlFamily::Generic, None, &[]) {
            Ok(s) => s,
            Err(_) => return,
        };
        this.netlink_socket = Some(sock);

        // C stores the socket first, then records the resolved family id (a
        // negative value on failure leaves a bad family that later fails at
        // send-time). Mirror that: keep the socket regardless of resolution.
        this.netlink_family = match this
            .netlink_socket
            .as_mut()
            .unwrap()
            .resolve_genl_family(TASKSTATS_GENL_NAME)
        {
            Ok(family) => i32::from(family),
            Err(_) => -1,
        };
    }

    /// Port of `LibNl.c:149` (`void LibNl_destroyNetlinkSocket(LinuxProcessTable* this)`).
    ///
    /// Closes and frees the socket. `neli`'s `NlSocketHandle` closes its fd on
    /// `Drop`, so `nl_close` + `nl_socket_free` collapse to dropping the
    /// `Option`. Then `unload_libnl` (a no-op under static linking) mirrors the
    /// C teardown tail.
    pub fn LibNl_destroyNetlinkSocket(this: &mut LinuxProcessTable) {
        if this.netlink_socket.is_some() {
            this.netlink_socket = None;
        }

        unload_libnl();
    }

    /// Port of `LibNl.c:161` (`static int handleNetlinkMsg(struct nl_msg* nlmsg, void* linuxProcess)`).
    ///
    /// Parses the `struct taskstats` out of the reply and fills the
    /// delay-accounting fields on `lp`. Upstream is a libnl `NL_CB_VALID`
    /// callback returning `NL_OK` / `NL_SKIP`; here it is called directly by
    /// [`LibNl_readDelayAcctData`] on the received message, so the
    /// dispatcher-only return code is dropped (unit return).
    ///
    /// The C reads the second nested attribute of `TASKSTATS_TYPE_AGGR_PID`
    /// (`nla_data(nla_next(nla_data(nlattr)))`), which is
    /// `TASKSTATS_TYPE_STATS`; `neli`'s nested-attribute lookup fetches that
    /// `STATS` payload by type directly.
    pub fn handleNetlinkMsg(nlmsg: &Nlmsghdr<u16, Genlmsghdr<u8, u16>>, lp: &mut LinuxProcess) {
        // struct taskstats — <linux/taskstats.h> (uapi). repr(C) with natural
        // alignment matches the kernel's (non-packed) layout on the SysV /
        // AArch64 ABIs, so field offsets line up with the wire bytes.
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct taskstats {
            version: u16,
            ac_exitcode: u32,
            ac_flag: u8,
            ac_nice: u8,
            cpu_count: u64,
            cpu_delay_total: u64,
            blkio_count: u64,
            blkio_delay_total: u64,
            swapin_count: u64,
            swapin_delay_total: u64,
            cpu_run_real_total: u64,
            cpu_run_virtual_total: u64,
            ac_comm: [u8; 32],
            ac_sched: u8,
            ac_pad: [u8; 3],
            ac_uid: u32,
            ac_gid: u32,
            ac_pid: u32,
            ac_ppid: u32,
            ac_btime: u32,
            ac_etime: u64,
            ac_utime: u64,
            ac_stime: u64,
            ac_minflt: u64,
            ac_majflt: u64,
            coremem: u64,
            virtmem: u64,
            hiwater_rss: u64,
            hiwater_vm: u64,
            read_char: u64,
            write_char: u64,
            read_syscalls: u64,
            write_syscalls: u64,
            read_bytes: u64,
            write_bytes: u64,
            cancelled_write_bytes: u64,
            nvcsw: u64,
            nivcsw: u64,
            ac_utimescaled: u64,
            ac_stimescaled: u64,
            cpu_scaled_run_real_total: u64,
            freepages_count: u64,
            freepages_delay_total: u64,
            thrashing_count: u64,
            thrashing_delay_total: u64,
            ac_btime64: u64,
            compact_count: u64,
            compact_delay_total: u64,
            ac_tgid: u32,
            ac_tgetime: u64,
            ac_exe_dev: u64,
            ac_exe_inode: u64,
            wpcopy_count: u64,
            wpcopy_delay_total: u64,
            irq_count: u64,
            irq_delay_total: u64,
        }

        let genl = match nlmsg.get_payload() {
            Ok(p) => p,
            Err(_) => return,
        };

        // genlmsg_parse into the top-level attribute table.
        let mut attrs = genl.get_attr_handle();

        // (nlattr = nlattrs[TASKSTATS_TYPE_AGGR_PID]) || (nlattr = nlattrs[TASKSTATS_TYPE_NULL])
        let nested = match attrs.get_nested_attributes::<u16>(TASKSTATS_TYPE_AGGR_PID) {
            Ok(h) => Some(h),
            Err(_) => attrs.get_nested_attributes::<u16>(TASKSTATS_TYPE_NULL).ok(),
        };
        let nested = match nested {
            Some(h) => h,
            None => return,
        };

        // nla_data(nla_next(nla_data(nlattr))) — the STATS payload.
        let payload = match nested.get_attr_payload_as_with_len::<Buffer>(TASKSTATS_TYPE_STATS) {
            Ok(b) => b,
            Err(_) => return,
        };

        // memcpy(&stats, ..., sizeof(stats)) — bounded by the bytes the kernel
        // actually sent (its taskstats may be an older/shorter version).
        let bytes: &[u8] = payload.as_ref();
        let mut stats: taskstats = unsafe { std::mem::zeroed() };
        let n = std::cmp::min(bytes.len(), std::mem::size_of::<taskstats>());
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                (&mut stats as *mut taskstats).cast::<u8>(),
                n,
            );
        }

        debug_assert_eq!(Process_getPid(&lp.super_), stats.ac_pid as i32);

        // The xxx_delay_total values wrap around on overflow.
        // (Linux Kernel "Documentation/accounting/taskstats-struct.rst")
        let time_delta = stats
            .ac_etime
            .wrapping_mul(1000)
            .wrapping_sub(lp.delay_read_time);
        // #define DELTAPERC(x, y) (timeDelta ? MINIMUM((float)((x)-(y)) / timeDelta * 100.0F, 100.0F) : NAN)
        let deltaperc = |x: u64, y: u64| -> f32 {
            if time_delta != 0 {
                (x.wrapping_sub(y) as f32 / time_delta as f32 * 100.0f32).min(100.0f32)
            } else {
                f32::NAN
            }
        };
        lp.cpu_delay_percent = deltaperc(stats.cpu_delay_total, lp.cpu_delay_total);
        lp.blkio_delay_percent = deltaperc(stats.blkio_delay_total, lp.blkio_delay_total);
        lp.swapin_delay_percent = deltaperc(stats.swapin_delay_total, lp.swapin_delay_total);

        lp.swapin_delay_total = stats.swapin_delay_total;
        lp.blkio_delay_total = stats.blkio_delay_total;
        lp.cpu_delay_total = stats.cpu_delay_total;
        lp.delay_read_time = stats.ac_etime.wrapping_mul(1000);
    }

    /// Port of `LibNl.c:199` (`void LibNl_readDelayAcctData(LinuxProcessTable* this, LinuxProcess* process)`).
    ///
    /// Gathers delay-accounting information for a single process: (re)opens the
    /// socket if needed, sends `TASKSTATS_CMD_GET` with the pid attribute, and
    /// dispatches the reply to [`handleNetlinkMsg`]. On any failure the three
    /// percentage fields are set to NaN (the C `delayacct_failure` label).
    pub fn LibNl_readDelayAcctData(this: &mut LinuxProcessTable, process: &mut LinuxProcess) {
        // Nested helper — the C `goto delayacct_failure` tail.
        fn delayacct_failure(process: &mut LinuxProcess) {
            process.swapin_delay_percent = f32::NAN;
            process.blkio_delay_percent = f32::NAN;
            process.cpu_delay_percent = f32::NAN;
        }

        if this.netlink_socket.is_none() {
            initNetlinkSocket(this);
            if this.netlink_socket.is_none() {
                delayacct_failure(process);
                return;
            }
        }

        let pid = Process_getPid(&process.super_);
        let family = this.netlink_family as u16;

        // genlmsg_put(..., family, 0, NLM_F_REQUEST, TASKSTATS_CMD_GET, TASKSTATS_VERSION)
        // + nla_put_u32(msg, TASKSTATS_CMD_ATTR_PID, pid)
        let mut attrs: GenlBuffer<u16, Buffer> = GenlBuffer::new();
        match Nlattr::new(false, false, TASKSTATS_CMD_ATTR_PID, pid as u32) {
            Ok(attr) => attrs.push(attr),
            Err(_) => {
                delayacct_failure(process);
                return;
            }
        }
        let genlhdr = Genlmsghdr::<u8, u16>::new(TASKSTATS_CMD_GET, TASKSTATS_VERSION, attrs);
        let nlhdr = Nlmsghdr::new(
            None,
            family,
            NlmFFlags::new(&[NlmF::Request]),
            None,
            None,
            NlPayload::Payload(genlhdr),
        );

        let sock = this.netlink_socket.as_mut().unwrap();

        // nl_send_sync(this->netlink_socket, msg)
        if sock.send(nlhdr).is_err() {
            delayacct_failure(process);
            return;
        }

        // nl_recvmsgs_default(this->netlink_socket) — drive the reply into the
        // handler (the C NL_CB_VALID callback).
        match sock.recv::<u16, Genlmsghdr<u8, u16>>() {
            Ok(Some(msg)) => handleNetlinkMsg(&msg, process),
            Ok(None) => {}
            Err(_) => {
                delayacct_failure(process);
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub use fallback::*;

#[cfg(not(target_os = "linux"))]
mod fallback {
    //! `HAVE_DELAYACCT`-off variant: upstream omits `LibNl.c` entirely on
    //! platforms without kernel delay-accounting. These bodies exist only so
    //! the shared build compiles; they carry no data.
    use crate::ported::linux::linuxprocess::LinuxProcess;
    use crate::ported::linux::linuxprocesstable::LinuxProcessTable;

    /// Port of `LibNl.c:77` — no libnl to load off-Linux; reports failure.
    pub fn load_libnl() -> i32 {
        -1
    }

    /// Port of `LibNl.c:48` — nothing to unload off-Linux.
    pub fn unload_libnl() {}

    /// Port of `LibNl.c:134` — no netlink socket off-Linux.
    pub fn initNetlinkSocket(_this: &mut LinuxProcessTable) {}

    /// Port of `LibNl.c:149` — no netlink socket to destroy off-Linux.
    pub fn LibNl_destroyNetlinkSocket(_this: &mut LinuxProcessTable) {}

    /// Port of `LibNl.c:161` — no taskstats message off-Linux.
    pub fn handleNetlinkMsg(_lp: &mut LinuxProcess) {}

    /// Port of `LibNl.c:199` — no delay-accounting data off-Linux; mirrors the
    /// C `delayacct_failure` label (NaN percentages).
    pub fn LibNl_readDelayAcctData(_this: &mut LinuxProcessTable, process: &mut LinuxProcess) {
        process.swapin_delay_percent = f32::NAN;
        process.blkio_delay_percent = f32::NAN;
        process.cpu_delay_percent = f32::NAN;
    }
}
