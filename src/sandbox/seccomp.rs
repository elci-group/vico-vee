#[repr(C)]
struct SockFilter {
    code: u16,
    jt: u8,
    jf: u8,
    k: u32,
}

#[repr(C)]
struct SockFprog {
    len: u16,
    filter: *const SockFilter,
}

const _SECCOMP_MODE_FILTER: u32 = 2;
const SECCOMP_SET_MODE_FILTER: i32 = 1;
const SECCOMP_FILTER_FLAG_TSYNC: u32 = 1;

// BPF opcodes
const BPF_LD: u16 = 0x00;
const BPF_W: u16 = 0x00;
const BPF_ABS: u16 = 0x20;
const BPF_JMP: u16 = 0x05;
const BPF_JEQ: u16 = 0x10;
const BPF_K: u16 = 0x00;
const BPF_RET: u16 = 0x06;
const BPF_ALLOW: u32 = 0x7fff_0000;
const BPF_KILL: u32 = 0x0000_0000;

fn bpf_stmt(code: u16, k: u32) -> SockFilter {
    SockFilter {
        code,
        jt: 0,
        jf: 0,
        k,
    }
}

fn bpf_jump(code: u16, k: u32, jt: u8, jf: u8) -> SockFilter {
    SockFilter { code, jt, jf, k }
}

/// Syscall numbers for x86_64
#[allow(dead_code)]
mod syscalls {
    pub const READ: u32 = 0;
    pub const WRITE: u32 = 1;
    pub const OPEN: u32 = 2;
    pub const CLOSE: u32 = 3;
    pub const STAT: u32 = 4;
    pub const FSTAT: u32 = 5;
    pub const LSTAT: u32 = 6;
    pub const POLL: u32 = 7;
    pub const LSEEK: u32 = 8;
    pub const MMAP: u32 = 9;
    pub const MPROTECT: u32 = 10;
    pub const MUNMAP: u32 = 11;
    pub const BRK: u32 = 12;
    pub const RT_SIGACTION: u32 = 13;
    pub const RT_SIGPROCMASK: u32 = 14;
    pub const RT_SIGRETURN: u32 = 15;
    pub const IOCTL: u32 = 16;
    pub const PREAD64: u32 = 17;
    pub const PWRITE64: u32 = 18;
    pub const READV: u32 = 19;
    pub const WRITEV: u32 = 20;
    pub const ACCESS: u32 = 21;
    pub const PIPE: u32 = 22;
    pub const SELECT: u32 = 23;
    pub const SCHED_YIELD: u32 = 24;
    pub const MREMAP: u32 = 25;
    pub const MSYNC: u32 = 26;
    pub const MINCORE: u32 = 27;
    pub const MADVISE: u32 = 28;
    pub const SHMGET: u32 = 29;
    pub const SHMAT: u32 = 30;
    pub const SHMCTL: u32 = 31;
    pub const DUP: u32 = 32;
    pub const DUP2: u32 = 33;
    pub const PAUSE: u32 = 34;
    pub const NANOSLEEP: u32 = 35;
    pub const GETITIMER: u32 = 36;
    pub const ALARM: u32 = 37;
    pub const SETITIMER: u32 = 38;
    pub const GETPID: u32 = 39;
    pub const SENDFILE: u32 = 40;
    pub const SOCKET: u32 = 41;
    pub const CONNECT: u32 = 42;
    pub const ACCEPT: u32 = 43;
    pub const SENDTO: u32 = 44;
    pub const RECVFROM: u32 = 45;
    pub const SENDMSG: u32 = 46;
    pub const RECVMSG: u32 = 47;
    pub const SHUTDOWN: u32 = 48;
    pub const BIND: u32 = 49;
    pub const LISTEN: u32 = 50;
    pub const GETSOCKNAME: u32 = 51;
    pub const GETPEERNAME: u32 = 52;
    pub const SOCKETPAIR: u32 = 53;
    pub const SETSOCKOPT: u32 = 54;
    pub const GETSOCKOPT: u32 = 55;
    pub const CLONE: u32 = 56;
    pub const FORK: u32 = 57;
    pub const VFORK: u32 = 58;
    pub const EXECVE: u32 = 59;
    pub const EXIT: u32 = 60;
    pub const WAIT4: u32 = 61;
    pub const KILL: u32 = 62;
    pub const UNAME: u32 = 63;
    pub const SEMGET: u32 = 64;
    pub const SEMOP: u32 = 65;
    pub const SEMCTL: u32 = 66;
    pub const SHMDT: u32 = 67;
    pub const MSGGET: u32 = 68;
    pub const MSGSND: u32 = 69;
    pub const MSGRCV: u32 = 70;
    pub const MSGCTL: u32 = 71;
    pub const FCNTL: u32 = 72;
    pub const FLOCK: u32 = 73;
    pub const FSYNC: u32 = 74;
    pub const FDATASYNC: u32 = 75;
    pub const TRUNCATE: u32 = 76;
    pub const FTRUNCATE: u32 = 77;
    pub const GETDENTS: u32 = 78;
    pub const GETCWD: u32 = 79;
    pub const CHDIR: u32 = 80;
    pub const FCHDIR: u32 = 81;
    pub const RENAME: u32 = 82;
    pub const MKDIR: u32 = 83;
    pub const RMDIR: u32 = 84;
    pub const CREAT: u32 = 85;
    pub const LINK: u32 = 86;
    pub const UNLINK: u32 = 87;
    pub const SYMLINK: u32 = 88;
    pub const READLINK: u32 = 89;
    pub const CHMOD: u32 = 90;
    pub const FCHMOD: u32 = 91;
    pub const CHOWN: u32 = 92;
    pub const FCHOWN: u32 = 93;
    pub const LCHOWN: u32 = 94;
    pub const UMASK: u32 = 95;
    pub const GETTIMEOFDAY: u32 = 96;
    pub const GETRLIMIT: u32 = 97;
    pub const GETRUSAGE: u32 = 98;
    pub const SYSINFO: u32 = 99;
    pub const TIMES: u32 = 100;
    pub const PTRACE: u32 = 101;
    pub const GETUID: u32 = 102;
    pub const SYSLOG: u32 = 103;
    pub const GETGID: u32 = 104;
    pub const SETUID: u32 = 105;
    pub const SETGID: u32 = 106;
    pub const GETEUID: u32 = 107;
    pub const GETEGID: u32 = 108;
    pub const SETPGID: u32 = 109;
    pub const GETPPID: u32 = 110;
    pub const GETPGRP: u32 = 111;
    pub const SETSID: u32 = 112;
    pub const SETREUID: u32 = 113;
    pub const SETREGID: u32 = 114;
    pub const GETGROUPS: u32 = 115;
    pub const SETGROUPS: u32 = 116;
    pub const SETRESUID: u32 = 117;
    pub const GETRESUID: u32 = 118;
    pub const SETRESGID: u32 = 119;
    pub const GETRESGID: u32 = 120;
    pub const GETPGID: u32 = 121;
    pub const SETFSUID: u32 = 122;
    pub const SETFSGID: u32 = 123;
    pub const GETSID: u32 = 124;
    pub const CAPGET: u32 = 125;
    pub const CAPSET: u32 = 126;
    pub const RT_SIGPENDING: u32 = 127;
    pub const RT_SIGTIMEDWAIT: u32 = 128;
    pub const RT_SIGQUEUEINFO: u32 = 129;
    pub const RT_SIGSUSPEND: u32 = 130;
    pub const SIGALTSTACK: u32 = 131;
    pub const UTIME: u32 = 132;
    pub const MKNOD: u32 = 133;
    pub const USELIB: u32 = 134;
    pub const PERSONALITY: u32 = 135;
    pub const USTAT: u32 = 136;
    pub const STATFS: u32 = 137;
    pub const FSTATFS: u32 = 138;
    pub const SYSFS: u32 = 139;
    pub const GETPRIORITY: u32 = 140;
    pub const SETPRIORITY: u32 = 141;
    pub const SCHED_SETPARAM: u32 = 142;
    pub const SCHED_GETPARAM: u32 = 143;
    pub const SCHED_SETSCHEDULER: u32 = 144;
    pub const SCHED_GETSCHEDULER: u32 = 145;
    pub const SCHED_GET_PRIORITY_MAX: u32 = 146;
    pub const SCHED_GET_PRIORITY_MIN: u32 = 147;
    pub const SCHED_RR_GET_INTERVAL: u32 = 148;
    pub const MLOCK: u32 = 149;
    pub const MUNLOCK: u32 = 150;
    pub const MLOCKALL: u32 = 151;
    pub const MUNLOCKALL: u32 = 152;
    pub const VHANGUP: u32 = 153;
    pub const MODIFY_LDT: u32 = 154;
    pub const PIVOT_ROOT: u32 = 155;
    pub const PRCTL: u32 = 157;
    pub const ARCH_PRCTL: u32 = 158;
    pub const ADJTIMEX: u32 = 159;
    pub const SETRLIMIT: u32 = 160;
    pub const CHROOT: u32 = 161;
    pub const SYNC: u32 = 162;
    pub const ACCT: u32 = 163;
    pub const SETTIMEOFDAY: u32 = 164;
    pub const MOUNT: u32 = 165;
    pub const UMOUNT2: u32 = 166;
    pub const SWAPON: u32 = 167;
    pub const SWAPOFF: u32 = 168;
    pub const REBOOT: u32 = 169;
    pub const SETHOSTNAME: u32 = 170;
    pub const SETDOMAINNAME: u32 = 171;
    pub const IOPL: u32 = 172;
    pub const IOPERM: u32 = 173;
    pub const CREATE_MODULE: u32 = 174;
    pub const INIT_MODULE: u32 = 175;
    pub const DELETE_MODULE: u32 = 176;
    pub const GET_KERNEL_SYMS: u32 = 177;
    pub const QUERY_MODULE: u32 = 178;
    pub const QUOTACTL: u32 = 179;
    pub const NFSSERVCTL: u32 = 180;
    pub const GETPMSG: u32 = 181;
    pub const PUTPMSG: u32 = 182;
    pub const AFS_SYSCALL: u32 = 183;
    pub const TUXCALL: u32 = 184;
    pub const SECURITY: u32 = 185;
    pub const GETTID: u32 = 186;
    pub const READAHEAD: u32 = 187;
    pub const SETXATTR: u32 = 188;
    pub const LSETXATTR: u32 = 189;
    pub const FSETXATTR: u32 = 190;
    pub const GETXATTR: u32 = 191;
    pub const LGETXATTR: u32 = 192;
    pub const FGETXATTR: u32 = 193;
    pub const LISTXATTR: u32 = 194;
    pub const LLISTXATTR: u32 = 195;
    pub const FLISTXATTR: u32 = 196;
    pub const REMOVEXATTR: u32 = 197;
    pub const LREMOVEXATTR: u32 = 198;
    pub const FREMOVEXATTR: u32 = 199;
    pub const TKILL: u32 = 200;
    pub const TIME: u32 = 201;
    pub const FUTEX: u32 = 202;
    pub const SCHED_SETAFFINITY: u32 = 203;
    pub const SCHED_GETAFFINITY: u32 = 204;
    pub const SET_THREAD_AREA: u32 = 205;
    pub const IO_SETUP: u32 = 206;
    pub const IO_DESTROY: u32 = 207;
    pub const IO_GETEVENTS: u32 = 208;
    pub const IO_SUBMIT: u32 = 209;
    pub const IO_CANCEL: u32 = 210;
    pub const GET_THREAD_AREA: u32 = 211;
    pub const LOOKUP_DCOOKIE: u32 = 212;
    pub const EPOLL_CREATE: u32 = 213;
    pub const EPOLL_CTL_OLD: u32 = 214;
    pub const EPOLL_WAIT_OLD: u32 = 215;
    pub const REMAP_FILE_PAGES: u32 = 216;
    pub const GETDENTS64: u32 = 217;
    pub const SET_TID_ADDRESS: u32 = 218;
    pub const RESTART_SYSCALL: u32 = 219;
    pub const SEMTIMEDOP: u32 = 220;
    pub const FADVISE64: u32 = 221;
    pub const TIMER_CREATE: u32 = 222;
    pub const TIMER_SETTIME: u32 = 223;
    pub const TIMER_GETTIME: u32 = 224;
    pub const TIMER_GETOVERRUN: u32 = 225;
    pub const TIMER_DELETE: u32 = 226;
    pub const CLOCK_SETTIME: u32 = 227;
    pub const CLOCK_GETTIME: u32 = 228;
    pub const CLOCK_GETRES: u32 = 229;
    pub const CLOCK_NANOSLEEP: u32 = 230;
    pub const EXIT_GROUP: u32 = 231;
    pub const EPOLL_WAIT: u32 = 232;
    pub const EPOLL_CTL: u32 = 233;
    pub const TGKILL: u32 = 234;
    pub const UTIMES: u32 = 235;
    pub const VSERVER: u32 = 236;
    pub const MBIND: u32 = 237;
    pub const SET_MEMPOLICY: u32 = 238;
    pub const GET_MEMPOLICY: u32 = 239;
    pub const MQ_OPEN: u32 = 240;
    pub const MQ_UNLINK: u32 = 241;
    pub const MQ_TIMEDSEND: u32 = 242;
    pub const MQ_TIMEDRECEIVE: u32 = 243;
    pub const MQ_NOTIFY: u32 = 244;
    pub const MQ_GETSETATTR: u32 = 245;
    pub const KEXEC_LOAD: u32 = 246;
    pub const WAITID: u32 = 247;
    pub const ADD_KEY: u32 = 248;
    pub const REQUEST_KEY: u32 = 249;
    pub const KEYCTL: u32 = 250;
    pub const IOPRIO_SET: u32 = 251;
    pub const IOPRIO_GET: u32 = 252;
    pub const INOTIFY_INIT: u32 = 253;
    pub const INOTIFY_ADD_WATCH: u32 = 254;
    pub const INOTIFY_RM_WATCH: u32 = 255;
    pub const MIGRATE_PAGES: u32 = 256;
    pub const OPENAT: u32 = 257;
    pub const MKDIRAT: u32 = 258;
    pub const MKNODAT: u32 = 259;
    pub const FCHOWNAT: u32 = 260;
    pub const FUTIMESAT: u32 = 261;
    pub const NEWFSTATAT: u32 = 262;
    pub const UNLINKAT: u32 = 263;
    pub const RENAMEAT: u32 = 264;
    pub const LINKAT: u32 = 265;
    pub const SYMLINKAT: u32 = 266;
    pub const READLINKAT: u32 = 267;
    pub const FCHMODAT: u32 = 268;
    pub const FACCESSAT: u32 = 269;
    pub const PSELECT6: u32 = 270;
    pub const PPOLL: u32 = 271;
    pub const UNSHARE: u32 = 272;
    pub const SET_ROBUST_LIST: u32 = 273;
    pub const GET_ROBUST_LIST: u32 = 274;
    pub const SPLICE: u32 = 275;
    pub const TEE: u32 = 276;
    pub const SYNC_FILE_RANGE: u32 = 277;
    pub const VMSPLICE: u32 = 278;
    pub const MOVE_PAGES: u32 = 279;
    pub const UTIMENSAT: u32 = 280;
    pub const EPOLL_PWAIT: u32 = 281;
    pub const SIGNALFD: u32 = 282;
    pub const TIMERFD_CREATE: u32 = 283;
    pub const EVENTFD: u32 = 284;
    pub const FALLOCATE: u32 = 285;
    pub const TIMERFD_SETTIME: u32 = 286;
    pub const TIMERFD_GETTIME: u32 = 287;
    pub const ACCEPT4: u32 = 288;
    pub const SIGNALFD4: u32 = 289;
    pub const EVENTFD2: u32 = 290;
    pub const EPOLL_CREATE1: u32 = 291;
    pub const DUP3: u32 = 292;
    pub const PIPE2: u32 = 293;
    pub const INOTIFY_INIT1: u32 = 294;
    pub const PREADV: u32 = 295;
    pub const PWRITEV: u32 = 296;
    pub const RT_TGSIGQUEUEINFO: u32 = 297;
    pub const PERF_EVENT_OPEN: u32 = 298;
    pub const RECVMMSG: u32 = 299;
    pub const FANOTIFY_INIT: u32 = 300;
    pub const FANOTIFY_MARK: u32 = 301;
    pub const PRLIMIT64: u32 = 302;
    pub const NAME_TO_HANDLE_AT: u32 = 303;
    pub const OPEN_BY_HANDLE_AT: u32 = 304;
    pub const CLOCK_ADJTIME: u32 = 305;
    pub const SYNCFS: u32 = 306;
    pub const SENDMMSG: u32 = 307;
    pub const SETNS: u32 = 308;
    pub const GETCPU: u32 = 309;
    pub const PROCESS_VM_READV: u32 = 310;
    pub const PROCESS_VM_WRITEV: u32 = 311;
    pub const KCMP: u32 = 312;
    pub const FINIT_MODULE: u32 = 313;
    pub const SCHED_SETATTR: u32 = 314;
    pub const SCHED_GETATTR: u32 = 315;
    pub const RENAMEAT2: u32 = 316;
    pub const SECCOMP: u32 = 317;
    pub const GETRANDOM: u32 = 318;
    pub const MEMFD_CREATE: u32 = 319;
    pub const KEXEC_FILE_LOAD: u32 = 320;
    pub const BPF: u32 = 321;
    pub const STUB_EXECVEAT: u32 = 322;
    pub const USERFAULTFD: u32 = 323;
    pub const MEMBARRIER: u32 = 324;
    pub const MLOCK2: u32 = 325;
    pub const COPY_FILE_RANGE: u32 = 326;
    pub const PREADV2: u32 = 327;
    pub const PWRITEV2: u32 = 328;
    pub const PKEY_MPROTECT: u32 = 329;
    pub const PKEY_ALLOC: u32 = 330;
    pub const PKEY_FREE: u32 = 331;
    pub const STATX: u32 = 332;
    pub const IO_PGETEVENTS: u32 = 333;
    pub const RSEQ: u32 = 334;
    pub const PIDFD_OPEN: u32 = 434;
    pub const CLONE3: u32 = 435;
    pub const OPENAT2: u32 = 437;
    pub const PIDFD_GETFD: u32 = 438;
    pub const FACCESSAT2: u32 = 439;
    pub const PROCESS_MADVISE: u32 = 440;
    pub const EPOLL_PWAIT2: u32 = 441;
    pub const MOUNT_SETATTR: u32 = 442;
    pub const QUOTACTL_FD: u32 = 443;
    pub const LANDLOCK_CREATE_RULESET: u32 = 444;
    pub const LANDLOCK_ADD_RULE: u32 = 445;
    pub const LANDLOCK_RESTRICT_SELF: u32 = 446;
    pub const MEMFD_SECRET: u32 = 447;
    pub const PROCESS_MRELEASE: u32 = 448;
}

pub(crate) fn apply_seccomp_filter(block_network: bool) -> Result<(), String> {
    let mut filter = vec![
        // Load syscall number into accumulator
        bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 0),
    ];

    // Allowlist of safe syscalls
    let allowed: &[u32] = &[
        syscalls::READ,
        syscalls::WRITE,
        syscalls::OPEN,
        syscalls::OPENAT,
        syscalls::CLOSE,
        syscalls::STAT,
        syscalls::FSTAT,
        syscalls::NEWFSTATAT,
        syscalls::STATFS,
        syscalls::FSTATFS,
        syscalls::FCHDIR,
        syscalls::LSEEK,
        syscalls::MMAP,
        syscalls::MPROTECT,
        syscalls::MUNMAP,
        syscalls::BRK,
        syscalls::RT_SIGACTION,
        syscalls::RT_SIGPROCMASK,
        syscalls::RT_SIGRETURN,
        syscalls::IOCTL,
        syscalls::PREAD64,
        syscalls::PWRITE64,
        syscalls::READV,
        syscalls::WRITEV,
        syscalls::ACCESS,
        syscalls::PIPE,
        syscalls::PIPE2,
        syscalls::DUP,
        syscalls::DUP2,
        syscalls::DUP3,
        syscalls::NANOSLEEP,
        syscalls::CLOCK_NANOSLEEP,
        syscalls::GETPID,
        syscalls::GETTID,
        syscalls::GETPPID,
        syscalls::GETUID,
        syscalls::GETEUID,
        syscalls::GETGID,
        syscalls::GETEGID,
        syscalls::EXIT,
        syscalls::EXIT_GROUP,
        syscalls::UNAME,
        syscalls::FCNTL,
        syscalls::FSYNC,
        syscalls::FDATASYNC,
        syscalls::GETCWD,
        syscalls::CHDIR,
        syscalls::RENAME,
        syscalls::RENAMEAT,
        syscalls::MKDIR,
        syscalls::MKDIRAT,
        syscalls::RMDIR,
        syscalls::CREAT,
        syscalls::UNLINK,
        syscalls::UNLINKAT,
        syscalls::READLINK,
        syscalls::READLINKAT,
        syscalls::GETTIMEOFDAY,
        syscalls::GETRLIMIT,
        syscalls::GETRUSAGE,
        syscalls::TIMES,
        syscalls::GETDENTS,
        syscalls::GETDENTS64,
        syscalls::FUTEX,
        syscalls::SET_TID_ADDRESS,
        syscalls::RESTART_SYSCALL,
        syscalls::PRLIMIT64,
        syscalls::SYSINFO,
        syscalls::MADVISE,
        syscalls::FADVISE64,
        syscalls::SCHED_YIELD,
        syscalls::FORK,
        syscalls::CLONE,
        syscalls::VFORK,
        syscalls::EXECVE,
        syscalls::STUB_EXECVEAT,
        syscalls::WAIT4,
        syscalls::ARCH_PRCTL,
        syscalls::SET_ROBUST_LIST,
        syscalls::GET_ROBUST_LIST,
        syscalls::PRCTL,
        syscalls::SECCOMP,
        syscalls::GETRANDOM,
        syscalls::MEMFD_CREATE,
        syscalls::COPY_FILE_RANGE,
        syscalls::FACCESSAT,
        syscalls::FACCESSAT2,
        syscalls::STATX,
        syscalls::CLONE3,
        syscalls::OPENAT2,
        syscalls::PREADV2,
        syscalls::PWRITEV2,
        syscalls::RSEQ,
    ];

    for &syscall in allowed {
        filter.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, syscall, 0, 1));
        filter.push(bpf_stmt(BPF_RET | BPF_K, BPF_ALLOW));
    }

    if !block_network {
        let net_syscalls: &[u32] = &[
            syscalls::SOCKET,
            syscalls::CONNECT,
            syscalls::ACCEPT,
            syscalls::ACCEPT4,
            syscalls::SENDTO,
            syscalls::RECVFROM,
            syscalls::SENDMSG,
            syscalls::RECVMSG,
            syscalls::SHUTDOWN,
            syscalls::BIND,
            syscalls::LISTEN,
            syscalls::GETSOCKNAME,
            syscalls::GETPEERNAME,
            syscalls::SOCKETPAIR,
            syscalls::SETSOCKOPT,
            syscalls::GETSOCKOPT,
        ];
        for &syscall in net_syscalls {
            filter.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, syscall, 0, 1));
            filter.push(bpf_stmt(BPF_RET | BPF_K, BPF_ALLOW));
        }
    }

    // Deny everything else
    filter.push(bpf_stmt(BPF_RET | BPF_K, BPF_KILL));

    let prog = SockFprog {
        len: filter.len() as u16,
        filter: filter.as_ptr(),
    };

    let ret = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    if ret < 0 {
        return Err("prctl(PR_SET_NO_NEW_PRIVS) failed".into());
    }

    let ret = unsafe {
        libc::syscall(
            syscalls::SECCOMP as i64,
            SECCOMP_SET_MODE_FILTER,
            SECCOMP_FILTER_FLAG_TSYNC,
            &prog,
        )
    };

    if ret < 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("seccomp filter failed: {}", err));
    }

    Ok(())
}
