//! Stub scaffold for `GPU.c` — NOT yet ported.
//!
//! Every `pub fn` below is a placeholder (`todo!()`) named after a real
//! htop C function so the port-purity gate accepts the module and the
//! port surface is laid out. Replace each stub with a faithful port of
//! the C body, updating the signature and the doc comment to `Port of
//! `GPU.c`:<line>.` as you go. `gen_port_report.py` counts these
//! `todo!()` bodies as *stubbed*, not *ported*, so scaffolding does not
//! inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]


/// TODO: port of `static bool is_duplicate_client(const ClientInfo* parsed, ClientID id, const char* pdev` from `GPU.c:38`.
pub fn is_duplicate_client() {
    todo!("port of GPU.c:38")
}

/// TODO: port of `static void update_machine_gpu(LinuxProcessTable* lpt, unsigned long long int time, const char* engine, size_t engine_len` from `GPU.c:48`.
pub fn update_machine_gpu() {
    todo!("port of GPU.c:48")
}

/// TODO: port of `void GPU_readProcessData(LinuxProcessTable* lpt, LinuxProcess* lp, openat_arg_t procFd` from `GPU.c:80`.
pub fn GPU_readProcessData() {
    todo!("port of GPU.c:80")
}
