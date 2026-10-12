use std::ffi::CString;
use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::process::exit;

use rustix::fs::{mkdir, symlink, unlink, Mode};
use rustix::mount::{mount, MountFlags};
use rustix::process::getpid;

// 引入 libc 调用系统调用
use libc::{c_char, c_int, syscall};

// finit_module 系统调用号 (ARM64 架构下为 313)
const SYS_FINIT_MODULE: libc::c_long = 313;

fn main() {
    // 1. 确保自己是 PID 1 (内核启动的第一个用户进程)
    if !getpid().is_init() {
        eprintln!("FATAL: Must be run as PID 1!");
        exit(1);
    }

    // 2. 挂载 /proc (加载内核模块必须要用到)
    mkdir("/proc", Mode::from_raw_mode(0o755)).ok();
    // 【关键修复】显式指定 None 的类型为 Option<&CStr>
    if let Err(e) = mount("proc", "/proc", "proc", MountFlags::empty(), None::<&std::ffi::CStr>) {
        eprintln!("FATAL: Cannot mount /proc: {}", e);
    }

    // 3. 加载 /uid_priv.ko
    let ko_path = "/uid_priv.ko";
    match load_module(ko_path) {
        Ok(_) => println!("SUCCESS: {} loaded into kernel!", ko_path),
        Err(e) => eprintln!("ERROR: Failed to load {}: {}", ko_path, e),
    }

    // 4. 处理系统原版 init (原版 init 被改名为 init.real)
    unlink("/init").ok();
    
    let real_init = if std::path::Path::new("/init.real").exists() {
        "/init.real"
    } else {
        "/system/bin/init" // 兜底
    };
    
    symlink(real_init, "/init").ok();

    // 5. 交接控制权
    println!("Transferring control to {}", real_init);
    let init_cstr = CString::new(real_init).unwrap();
    let argv = [init_cstr.as_ptr(), std::ptr::null()];
    
    unsafe {
        // execve 成功的话，下面的代码永远不会执行
        libc::execve(init_cstr.as_ptr(), argv.as_ptr(), std::ptr::null());
    }
    
    eprintln!("FATAL: Failed to exec {}", real_init);
    exit(1);
}

/// 调用 finit_module 加载 .ko
fn load_module(path: &str) -> Result<(), String> {
    let file = File::open(path).map_err(|e| format!("Cannot open file: {}", e))?;
    let fd = file.as_raw_fd();
    
    // 直接系统调用加载模块，绕过缺失 insmod 的限制
    let ret = unsafe {
        syscall(
            SYS_FINIT_MODULE,
            fd,
            "".as_ptr() as *const c_char, // 给模块传的参数（空）
            0 as c_int,                   // flags
        )
    };

    if ret == 0 {
        Ok(())
    } else {
        let errno = std::io::Error::last_os_error();
        Err(format!("finit_module failed: {} (errno: {})", errno, errno.raw_os_error().unwrap_or(0)))
    }
}
