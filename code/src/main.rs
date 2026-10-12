use std::ffi::CString;
use std::fs::{self, File};
use std::os::unix::io::AsRawFd;
use std::process::exit;

// 只使用 libc，完全避开 rustix 的类型陷阱
use libc::{c_char, c_int, c_ulong, syscall};

// finit_module 系统调用号 (ARM64 架构下为 313)
const SYS_FINIT_MODULE: libc::c_long = 313;

fn main() {
    // 1. 确保自己是 PID 1 (Android 的 init 必须是 1 号进程)
    // 直接使用 libc 获取 PID
    if unsafe { libc::getpid() } != 1 {
        eprintln!("FATAL: Must be run as PID 1!");
        exit(1);
    }

    // 2. 挂载 /proc (使用 libc 原生 mount，不用 rustix)
    fs::create_dir_all("/proc").ok();
    let source = CString::new("proc").unwrap();
    let target = CString::new("/proc").unwrap();
    let fstype = CString::new("proc").unwrap();
    
    let ret = unsafe {
        libc::mount(
            source.as_ptr(),
            target.as_ptr(),
            fstype.as_ptr(),
            0 as c_ulong,
            std::ptr::null(), // 数据参数直接传空指针，完美解决类型推断问题
        )
    };
    if ret != 0 {
        eprintln!("FATAL: Cannot mount /proc");
    }

    // 3. 加载 /uid_priv.ko
    let ko_path = "/uid_priv.ko";
    match load_module(ko_path) {
        Ok(_) => println!("SUCCESS: {} loaded into kernel!", ko_path),
        Err(e) => eprintln!("ERROR: Failed to load {}: {}", ko_path, e),
    }

    // 4. 处理系统原版 init 交接逻辑
    fs::remove_file("/init").ok(); // 删除我们自己的 /init 软链接

    let real_init = if fs::metadata("/init.real").is_ok() {
        "/init.real"
    } else {
        "/system/bin/init" // 兜底
    };

    // 重新创建软链接，让系统后续能找到真正的 init
    std::os::unix::fs::symlink(real_init, "/init").ok();

    // 5. 交接控制权给真正的系统 init
    println!("Transferring control to {}", real_init);
    let init_cstr = CString::new(real_init).unwrap();
    let argv = [init_cstr.as_ptr(), std::ptr::null()];

    unsafe {
        // execve 成功的话，下面的代码永远不会执行
        libc::execve(init_cstr.as_ptr(), argv.as_ptr(), std::ptr::null());
    }

    // 如果走到这里说明交接失败
    eprintln!("FATAL: Failed to exec {}", real_init);
    exit(1);
}

/// 调用 finit_module 加载 .ko
fn load_module(path: &str) -> Result<(), String> {
    let file = File::open(path).map_err(|e| format!("Cannot open file: {}", e))?;
    let fd = file.as_raw_fd();

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
        Err(format!(
            "finit_module failed: {} (errno: {})",
            errno,
            errno.raw_os_error().unwrap_or(0)
        ))
    }
}
