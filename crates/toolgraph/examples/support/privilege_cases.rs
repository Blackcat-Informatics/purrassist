// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Declared ABI vocabulary with native capability ordinals and dangerous grants.

// Facts from the ABI 3.0 caps/mask and Linux capability-number contract;
// qualification checks actual native low/high words, not this table alone.
pub(crate) const CAPABILITIES: &[(&str, u8, bool)] = &[
    ("chown", 0, true),
    ("dac_override", 1, true),
    ("dac_read_search", 2, true),
    ("fowner", 3, true),
    ("fsetid", 4, false),
    ("kill", 5, true),
    ("setgid", 6, true),
    ("setuid", 7, true),
    ("setpcap", 8, false),
    ("linux_immutable", 9, false),
    ("net_bind_service", 10, true),
    ("net_broadcast", 11, false),
    ("net_admin", 12, true),
    ("net_raw", 13, false),
    ("ipc_lock", 14, false),
    ("ipc_owner", 15, false),
    ("sys_module", 16, true),
    ("sys_rawio", 17, true),
    ("sys_chroot", 18, false),
    ("sys_ptrace", 19, true),
    ("sys_pacct", 20, false),
    ("sys_admin", 21, true),
    ("sys_boot", 22, false),
    ("sys_nice", 23, false),
    ("sys_resource", 24, false),
    ("sys_time", 25, false),
    ("sys_tty_config", 26, false),
    ("mknod", 27, true),
    ("lease", 28, false),
    ("audit_write", 29, false),
    ("audit_control", 30, false),
    ("setfcap", 31, false),
    ("mac_override", 32, false),
    ("mac_admin", 33, false),
    ("syslog", 34, false),
    ("wake_alarm", 35, false),
    ("block_suspend", 36, false),
    ("audit_read", 37, false),
    ("perfmon", 38, false),
    ("bpf", 39, false),
];

pub(crate) const BASELINE: &str = "deny capability sys_admin,\n\
deny capability sys_ptrace,\n\
deny capability sys_module,\n\
deny capability sys_rawio,\n\
deny capability net_admin,\n\
deny capability dac_override,\n\
deny capability dac_read_search,\n\
deny capability setuid,\n\
deny capability setgid,\n\
deny capability mknod,\n\
deny capability chown,\n\
deny capability fowner,\n\
deny capability kill,\n\
deny capability net_bind_service,\n";
