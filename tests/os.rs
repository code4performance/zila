//! Zila Lang OS: the boot sector and kernel are Zila Lang programs. Building the
//! image needs no external tool; booting it needs QEMU (the test is skipped
//! when QEMU is not installed).

use zila::codegen_x64::build_os;
use zila::parse_source;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

fn image() -> Vec<u8> {
    let boot = parse_source(&std::fs::read_to_string("os/boot.br").unwrap()).unwrap();
    let kernel = parse_source(&std::fs::read_to_string("os/kernel.br").unwrap()).unwrap();
    build_os(&boot, &kernel).unwrap()
}

#[test]
fn image_has_a_valid_boot_sector() {
    let img = image();
    assert_eq!(img.len() % 512, 0);
    assert_eq!(&img[510..512], [0x55, 0xAA]);
    // the builder stored the kernel size (in sectors) in the boot sector
    let sectors = (img.len() / 512 - 1) as u16;
    assert!(img.windows(2).any(|w| w == sectors.to_le_bytes()));
    // the kernel starts with its entry point: `cli; cld`
    assert_eq!(&img[512..514], [0xFA, 0xFC]);
}

#[test]
fn kernel_without_console_write_is_rejected() {
    let boot = parse_source(&std::fs::read_to_string("os/boot.br").unwrap()).unwrap();
    let kernel = parse_source("package main\nfunc main()\nend\n").unwrap();
    let e = build_os(&boot, &kernel).unwrap_err();
    assert!(e.contains("console_write"), "{}", e);
}

#[test]
fn windows_only_libraries_are_rejected() {
    let boot = parse_source(&std::fs::read_to_string("os/boot.br").unwrap()).unwrap();
    let kernel = parse_source("package main\nuse ui\nfunc console_write(s: text)\nend\nfunc main()\nend\n").unwrap();
    let e = build_os(&boot, &kernel).unwrap_err();
    assert!(e.contains("bare-metal"), "{}", e);
}

fn find_qemu() -> Option<String> {
    let candidates = [
        "qemu-system-x86_64".to_string(),
        format!("{}/qemu/qemu-system-x86_64.exe", std::env::var("USERPROFILE").unwrap_or_default()),
        r"C:\Program Files\qemu\qemu-system-x86_64.exe".to_string(),
    ];
    candidates.into_iter().find(|c| Command::new(c).arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().map_or(false, |s| s.success()))
}

#[test]
fn boots_in_qemu_and_runs_shell_commands() {
    let Some(qemu) = find_qemu() else {
        eprintln!("QEMU not found: skipping the boot test");
        return;
    };
    let path = std::env::temp_dir().join(format!("zila-os-test-{}.img", std::process::id()));
    std::fs::write(&path, image()).unwrap();
    let mut child = Command::new(qemu)
        .args(["-drive", &format!("format=raw,file={}", path.display()), "-serial", "stdio", "-display", "none", "-no-reboot", "-m", "128", "-monitor", "none"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(3));
        for cmd in ["calc 6 * 7", "calc 1 / 0", "calc abc + 1", "bogus", "mem"] {
            let _ = writeln!(stdin, "{}", cmd);
            let _ = stdin.flush();
            std::thread::sleep(Duration::from_millis(800));
        }
        std::thread::sleep(Duration::from_secs(1));
        drop(stdin);
    });
    let killer = {
        let id = child.id();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(14));
            let _ = Command::new("taskkill").args(["/F", "/PID", &id.to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).status();
        })
    };
    let out = child.wait_with_output().unwrap();
    let _ = writer.join();
    drop(killer);
    let _ = std::fs::remove_file(&path);
    let text = String::from_utf8_lossy(&out.stdout).replace('\r', "");
    for want in [
        "Zila Lang OS 0.1",
        "6 * 7 = 42",
        "division by zero",
        "error: 'abc' is not a number",
        "error: unknown command 'bogus'",
        "heap used    : ",
        "live objects : 2",
    ] {
        assert!(text.contains(want), "missing {:?} in:\n{}", want, text);
    }
}

#[test]
fn kernels_cannot_use_async_or_the_windows_libraries() {
    let boot = parse_source(&std::fs::read_to_string("os/boot.br").unwrap()).unwrap();
    for body in [
        "async func f() -> int\n    return 1\nend\n",
        "func g() -> int\n    return await sleep(1)\nend\n",
    ] {
        let src = format!("package main\n{}func console_write(s: text)\nend\nfunc main()\nend\n", body);
        let e = build_os(&boot, &parse_source(&src).unwrap()).unwrap_err();
        assert!(e.contains("async") && e.contains("Windows"), "{}", e);
    }
    for lib in ["fs", "net", "future", "task"] {
        let src = format!("package main\nuse {}\nfunc console_write(s: text)\nend\nfunc main()\nend\n", lib);
        let e = build_os(&boot, &parse_source(&src).unwrap()).unwrap_err();
        assert!(e.contains("bare-metal"), "{}: {}", lib, e);
    }
}

#[test]
fn the_kernel_supplies_its_own_heap() {
    // the heap is `#[allocator]` code in os/kernel.br, so the runtime's built-in size-class heap is gone
    let kernel = parse_source(&std::fs::read_to_string("os/kernel.br").unwrap()).unwrap();
    let asm = zila::codegen_x64::generate_asm_for(&kernel, &zila::codegen_x64::CompileOptions::default(), zila::codegen_x64::Target::Bare).unwrap();
    assert!(asm.contains("call B_main_heap_alloc") && asm.contains("call B_main_heap_free"));
    assert!(!asm.contains("rt_freelists") && !asm.contains("rt_brk"));
}
