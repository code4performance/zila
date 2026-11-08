//! Part of the code generator (`codegen_x64`): see the section comment below.

use super::*;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Where the kernel is loaded (and linked to run) by the boot sector.
pub const KERNEL_BASE: u64 = 0x10000;
/// The boot sector lives where the BIOS puts it.
pub const BOOT_BASE: u64 = 0x7C00;
pub(super) const MAX_KERNEL_SECTORS: usize = 1000;

pub(super) fn collect_asm(b: &[Stmt], out: &mut Vec<String>) {
    for s in b {
        match &s.kind {
            StmtKind::Asm(a) => out.extend(a.lines.iter().cloned()),
            StmtKind::Unsafe(inner) => collect_asm(inner, out),
            _ => {}
        }
    }
}

/// The assembly written in the `#[naked]` functions of a boot file.
pub(super) fn boot_source(file: &File) -> Result<String, String> {
    let mut lines = Vec::new();
    for d in &file.decls {
        if let DeclKind::Func(fd) = &d.kind {
            if d.attrs.iter().any(|a| a.name == "naked") {
                collect_asm(&fd.body, &mut lines);
            }
        }
    }
    if lines.is_empty() {
        return Err(format!("{}.br: the boot file needs a #[naked] function containing `asm` blocks", file.package));
    }
    Ok(lines.join("
"))
}

/// Builds a bootable disk image: boot sector + kernel.
///
/// The boot file is Brasa whose `#[naked]` functions hold the real-mode and
/// mode-switching code; the kernel is ordinary Brasa compiled for long mode
/// and linked at `KERNEL_BASE`. It must define `func console_write(s: text)`,
/// which receives everything `print` writes.
pub fn build_os(boot: &File, kernel: &File) -> Result<Vec<u8>, String> {
    let mut c = Compiler::new(Target::Bare);
    c.load(kernel)?;
    c.declare()?;
    let ok = matches!(
        c.funcs.get(&(kernel.package.clone(), "console_write".to_string())),
        Some(s) if s.params.len() == 1 && s.params[0].ty == Ty::Text && s.ret == Ty::Void
    );
    if !ok {
        return Err(format!(
            "{}.br: the kernel must define `func console_write(s: text)`; `print` sends its output there",
            kernel.package
        ));
    }
    let asm = c.generate().map_err(crate::comptime::map_error)?;
    let k = crate::asm_x86::assemble(&asm, KERNEL_BASE, 64).map_err(|e| format!("kernel: {}", e))?;
    let mut kernel_bytes = k.bytes;
    while kernel_bytes.len() % 512 != 0 {
        kernel_bytes.push(0);
    }
    let sectors = kernel_bytes.len() / 512;
    if sectors > MAX_KERNEL_SECTORS {
        return Err(format!("the kernel is {} sectors; the boot sector loads at most {}", sectors, MAX_KERNEL_SECTORS));
    }

    let b = crate::asm_x86::assemble(&boot_source(boot)?, BOOT_BASE, 16).map_err(|e| format!("boot: {}", e))?;
    let mut boot_bytes = b.bytes;
    if boot_bytes.len() != 512 || boot_bytes[510..] != [0x55, 0xAA] {
        return Err(format!(
            "boot: the boot sector must be exactly 512 bytes ending in 0x55 0xAA (it is {} bytes)",
            boot_bytes.len()
        ));
    }
    let Some(&at) = b.symbols.get("kernel_sectors") else {
        return Err("boot: define `kernel_sectors dw 0`; the builder stores the kernel size there".into());
    };
    let off = (at - BOOT_BASE) as usize;
    boot_bytes[off..off + 2].copy_from_slice(&(sectors as u16).to_le_bytes());

    let mut image = boot_bytes;
    image.extend_from_slice(&kernel_bytes);
    Ok(image)
}

/// Compiles a parsed program (plus the std modules it uses) to MASM assembly.
pub fn generate_asm(file: &File) -> Result<String, String> {
    generate_asm_with(file, &CompileOptions::default())
}

/// `generate_asm` with explicit options.
pub fn generate_asm_with(file: &File, options: &CompileOptions) -> Result<String, String> {
    let mut c = Compiler::new(Target::Windows);
    c.opt = options.optimize;
    c.test_mode = options.test_mode;
    c.load(file).map_err(crate::comptime::map_error)?;
    c.declare().map_err(crate::comptime::map_error)?;
    c.generate().map_err(crate::comptime::map_error)
}

/// Where a Linux executable is loaded, and how much zeroed memory follows it for the small-object heap.
pub(super) const LINUX_BASE: u64 = 0x400000;
pub(super) const LINUX_HEAP: u64 = 256 * 1024 * 1024;
pub(super) const ELF_HEADERS: u64 = 64 + 56;

/// A static ELF64 executable holding `code` (assembled at `LINUX_BASE + ELF_HEADERS`) that starts at
/// `entry`, with `LINUX_HEAP` bytes of zeroed memory after it (`p_memsz` beyond `p_filesz`).
pub fn elf_image(code: &[u8], entry: u64) -> Vec<u8> {
    let filesz = ELF_HEADERS + code.len() as u64;
    let memsz = (filesz + 4095) / 4096 * 4096 + LINUX_HEAP;
    let mut out = Vec::with_capacity(filesz as usize);
    out.extend_from_slice(&[0x7F, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    out.extend_from_slice(&0x3Eu16.to_le_bytes()); // x86-64
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&entry.to_le_bytes());
    out.extend_from_slice(&64u64.to_le_bytes()); // program headers follow the ELF header
    out.extend_from_slice(&0u64.to_le_bytes()); // no section headers
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&64u16.to_le_bytes());
    out.extend_from_slice(&56u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&64u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    // the one loadable segment: readable, writable, executable, from the start of the file
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());
    out.extend_from_slice(&LINUX_BASE.to_le_bytes());
    out.extend_from_slice(&LINUX_BASE.to_le_bytes());
    out.extend_from_slice(&filesz.to_le_bytes());
    out.extend_from_slice(&memsz.to_le_bytes());
    out.extend_from_slice(&4096u64.to_le_bytes());
    out.extend_from_slice(code);
    out
}

/// `generate_asm_with` for a given target.
pub fn generate_asm_for(file: &File, options: &CompileOptions, target: Target) -> Result<String, String> {
    let mut c = Compiler::new(target);
    c.opt = options.optimize;
    c.test_mode = options.test_mode;
    c.load(file).map_err(crate::comptime::map_error)?;
    c.declare().map_err(crate::comptime::map_error)?;
    c.generate().map_err(crate::comptime::map_error)
}

/// The entry C uses to call Brasa function `name`: its System V arguments (integers in rdi, rsi,
/// rdx, rcx, r8, r9 and floats in xmm0-7, in order of their kind) are moved to where Brasa
/// functions expect them (position i in rcx, rdx, r8, r9, then the stack; floats as bits in the
/// integer registers), and a float result goes to xmm0. `kinds`: 0 integer, 1 f32, 2 f64.
pub(super) fn callback_thunk(name: &str, kinds: &[u8], ret: u8) -> String {
    const IREGS: [&str; 6] = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
    let mut t = format!("CB_{} proc
    sub rsp, 120
", name);
    let (mut ni, mut nf) = (0, 0);
    for (i, k) in kinds.iter().enumerate().take(6) {
        match k {
            0 if ni < 6 => {
                t.push_str(&format!("    mov qword ptr [rsp + {}], {}
", 64 + 8 * i, IREGS[ni]));
                ni += 1;
            }
            1 if nf < 8 => {
                t.push_str(&format!("    movd eax, xmm{}
    mov qword ptr [rsp + {}], rax
", nf, 64 + 8 * i));
                nf += 1;
            }
            2 if nf < 8 => {
                t.push_str(&format!("    movq rax, xmm{}
    mov qword ptr [rsp + {}], rax
", nf, 64 + 8 * i));
                nf += 1;
            }
            _ => {}
        }
    }
    for i in 0..kinds.len().min(6) {
        if i < 4 {
            t.push_str(&format!("    mov {}, qword ptr [rsp + {}]
", ARG_REGS[i], 64 + 8 * i));
        } else {
            t.push_str(&format!("    mov rax, qword ptr [rsp + {}]
    mov qword ptr [rsp + {}], rax
", 64 + 8 * i, 32 + 8 * (i - 4)));
        }
    }
    t.push_str(&format!("    call {}
", name));
    match ret {
        1 => t.push_str("    movd xmm0, eax
"),
        2 => t.push_str("    movq xmm0, rax
"),
        _ => {}
    }
    t.push_str(&format!("    add rsp, 120
    ret
CB_{} endp
", name));
    t
}

/// The C functions a Linux program imports: the names of the `GOT_<name>` slots in its assembly.
pub(super) fn linux_imports(asm: &str) -> Vec<String> {
    asm.lines().filter_map(|l| l.strip_prefix("GOT_").and_then(|r| r.strip_suffix(" dq 0"))).map(|n| n.to_string()).collect()
}

/// The libraries a Linux program links (`; link <name>` lines of its assembly) as shared-object names.
pub(super) fn linux_needed(asm: &str) -> Vec<String> {
    asm.lines()
        .filter_map(|l| l.strip_prefix("; link "))
        .map(|n| match n.trim() {
            "c" => "libc.so.6".to_string(),
            "m" => "libm.so.6".to_string(),
            "dl" => "libdl.so.2".to_string(),
            "pthread" => "libpthread.so.0".to_string(),
            "rt" => "librt.so.1".to_string(),
            "ssl" => "libssl.so.3".to_string(),
            "crypto" => "libcrypto.so.3".to_string(),
            other => format!("lib{}.so", other),
        })
        .collect()
}

/// The part of a dynamically linked executable that comes before the code: the ELF header, the
/// program headers (interpreter, the one loadable segment, the dynamic section, a non-executable
/// stack), the interpreter name, the dynamic string and symbol tables, a hash table, the
/// relocations that fill the import slots, and the dynamic section. The size depends only on
/// the names, not on `slots` (the addresses of the import slots), `entry` or `code_len`.
pub fn elf_dynamic_head(imports: &[String], needed: &[String], slots: &[u64], entry: u64, code_len: u64) -> Vec<u8> {
    const INTERP: &[u8] = b"/lib64/ld-linux-x86-64.so.2\0";
    let phnum = 5u64;
    let interp_off = 64 + 56 * phnum;
    let str_off = (interp_off + INTERP.len() as u64 + 7) / 8 * 8;
    // dynamic strings: "\0", the libraries, then the imported names
    let mut strtab = vec![0u8];
    let name_at = |strtab: &mut Vec<u8>, n: &str| {
        let at = strtab.len() as u32;
        strtab.extend_from_slice(n.as_bytes());
        strtab.push(0);
        at
    };
    let needed_at: Vec<u32> = needed.iter().map(|n| name_at(&mut strtab, n)).collect();
    let import_at: Vec<u32> = imports.iter().map(|n| name_at(&mut strtab, n)).collect();
    let n = imports.len() as u64;
    let sym_off = (str_off + strtab.len() as u64 + 7) / 8 * 8;
    let hash_off = sym_off + 24 * (n + 1);
    let hash_len = 4 * (2 + 1 + (n + 1));
    let rela_off = (hash_off + hash_len + 7) / 8 * 8;
    let dyn_off = (rela_off + 24 * n + 15) / 16 * 16;
    let dyn_entries = needed.len() as u64 + 11;
    let head_len = dyn_off + 16 * dyn_entries;
    let filesz = (head_len + 15) / 16 * 16 + code_len;
    let memsz = (filesz + 4095) / 4096 * 4096 + LINUX_HEAP;
    let at = |off: u64| LINUX_BASE + off;

    let mut out = Vec::new();
    // ELF header
    out.extend_from_slice(&[0x7F, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&0x3Eu16.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&entry.to_le_bytes());
    out.extend_from_slice(&64u64.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&64u16.to_le_bytes());
    out.extend_from_slice(&56u16.to_le_bytes());
    out.extend_from_slice(&(phnum as u16).to_le_bytes());
    out.extend_from_slice(&64u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    let phdr = |out: &mut Vec<u8>, kind: u32, flags: u32, off: u64, filesz: u64, memsz: u64, align: u64| {
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&off.to_le_bytes());
        out.extend_from_slice(&at(off).to_le_bytes());
        out.extend_from_slice(&at(off).to_le_bytes());
        out.extend_from_slice(&filesz.to_le_bytes());
        out.extend_from_slice(&memsz.to_le_bytes());
        out.extend_from_slice(&align.to_le_bytes());
    };
    phdr(&mut out, 6, 4, 64, 56 * phnum, 56 * phnum, 8); // PT_PHDR
    phdr(&mut out, 3, 4, interp_off, INTERP.len() as u64, INTERP.len() as u64, 1); // PT_INTERP
    phdr(&mut out, 1, 7, 0, filesz, memsz, 4096); // PT_LOAD
    phdr(&mut out, 2, 6, dyn_off, 16 * dyn_entries, 16 * dyn_entries, 8); // PT_DYNAMIC
    // PT_GNU_STACK: no executable stack
    out.extend_from_slice(&0x6474E551u32.to_le_bytes());
    out.extend_from_slice(&6u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 48]);
    out.extend_from_slice(INTERP);
    out.resize(str_off as usize, 0);
    out.extend_from_slice(&strtab);
    out.resize(sym_off as usize, 0);
    // symbols: the null symbol, then one undefined global per import
    out.extend_from_slice(&[0u8; 24]);
    for &name in &import_at {
        out.extend_from_slice(&name.to_le_bytes());
        out.push(0x10); // STB_GLOBAL, STT_NOTYPE
        out.push(0);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
    }
    // hash table: one bucket chaining every symbol
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&((n + 1) as u32).to_le_bytes());
    out.extend_from_slice(&(if n > 0 { 1u32 } else { 0 }).to_le_bytes());
    for i in 0..=n {
        out.extend_from_slice(&(if i >= 1 && i < n { i as u32 + 1 } else { 0 }).to_le_bytes());
    }
    out.resize(rela_off as usize, 0);
    // R_X86_64_GLOB_DAT: the slot receives the address of the symbol
    for (i, slot) in slots.iter().enumerate() {
        out.extend_from_slice(&slot.to_le_bytes());
        out.extend_from_slice(&(((i as u64 + 1) << 32) | 6).to_le_bytes());
        out.extend_from_slice(&0i64.to_le_bytes());
    }
    out.resize(dyn_off as usize, 0);
    let dynamic = |out: &mut Vec<u8>, tag: u64, val: u64| {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&val.to_le_bytes());
    };
    for &name in &needed_at {
        dynamic(&mut out, 1, name as u64); // DT_NEEDED
    }
    dynamic(&mut out, 4, at(hash_off)); // DT_HASH
    dynamic(&mut out, 5, at(str_off)); // DT_STRTAB
    dynamic(&mut out, 6, at(sym_off)); // DT_SYMTAB
    dynamic(&mut out, 10, strtab.len() as u64); // DT_STRSZ
    dynamic(&mut out, 11, 24); // DT_SYMENT
    dynamic(&mut out, 7, at(rela_off)); // DT_RELA
    dynamic(&mut out, 8, 24 * n); // DT_RELASZ
    dynamic(&mut out, 9, 24); // DT_RELAENT
    dynamic(&mut out, 30, 8); // DT_FLAGS: DF_BIND_NOW
    dynamic(&mut out, 0, 0); // DT_NULL
    dynamic(&mut out, 0, 0);
    debug_assert_eq!(out.len() as u64, head_len);
    out.resize(((head_len + 15) / 16 * 16) as usize, 0);
    out
}

/// Compiles to a Linux x86-64 executable with the compiler's own assembler (no linker): a static
/// one that needs no library, or, when the program calls C functions, a dynamically linked one.
pub fn compile_to_linux(file: &File, output: &str, options: &CompileOptions) -> Result<(), String> {
    let asm = generate_asm_for(file, options, Target::Linux)?;
    let imports = linux_imports(&asm);
    let image = if imports.is_empty() {
        let assembled = crate::asm_x86::assemble(&asm, LINUX_BASE + ELF_HEADERS, 64)?;
        let Some(&entry) = assembled.symbols.get("brasa_start") else {
            return Err("internal error: no entry point".into());
        };
        elf_image(&assembled.bytes, entry)
    } else {
        let needed = linux_needed(&asm);
        let head_len = elf_dynamic_head(&imports, &needed, &vec![0; imports.len()], 0, 0).len() as u64;
        let assembled = crate::asm_x86::assemble(&asm, LINUX_BASE + head_len, 64)?;
        let Some(&entry) = assembled.symbols.get("brasa_start") else {
            return Err("internal error: no entry point".into());
        };
        let mut slots = Vec::new();
        for n in &imports {
            let Some(&a) = assembled.symbols.get(&format!("GOT_{}", n)) else {
                return Err(format!("internal error: no import slot for {}", n));
            };
            slots.push(a);
        }
        let mut image = elf_dynamic_head(&imports, &needed, &slots, entry, assembled.bytes.len() as u64);
        image.extend_from_slice(&assembled.bytes);
        image
    };
    std::fs::write(output, &image).map_err(|e| format!("cannot write {}: {}", output, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(output, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// What a symbol of the generated assembly belongs to, for the link report: the key of a module,
/// or one of the shared groups.
pub(super) fn symbol_owner(sym: &str, modules: &[(String, String)]) -> Option<(String, bool)> {
    // (owner, is_code)
    let in_module = |rest: &str| -> Option<String> {
        modules.iter().filter(|(_, s)| rest.starts_with(&format!("{}_", s)) || rest == s.as_str()).max_by_key(|(_, s)| s.len()).map(|(k, _)| k.clone())
    };
    if let Some(rest) = sym.strip_prefix("B_init_") {
        return Some((in_module(rest).unwrap_or_else(|| "(helpers)".into()), true));
    }
    if sym.starts_with("B_drop_") || sym.starts_with("B_copy_") || sym.starts_with("B_eq_") || sym.starts_with("B_hash_") {
        return Some(("(helpers)".into(), true));
    }
    if let Some(rest) = sym.strip_prefix("B_") {
        return Some((in_module(rest).unwrap_or_else(|| "(helpers)".into()), true));
    }
    if let Some(rest) = sym.strip_prefix("G_") {
        return Some((in_module(rest).unwrap_or_else(|| "(helpers)".into()), false));
    }
    if sym.starts_with("S_") {
        return Some(("(strings)".into(), false));
    }
    if sym.starts_with("E_") || sym.starts_with("C_") {
        return Some(("(helpers)".into(), false));
    }
    if sym.starts_with("rt_msg_") || sym == "rt_heap" || sym == "rt_image_end" {
        return Some(("(runtime)".into(), false));
    }
    if sym.starts_with("rt_") || sym.starts_with("CB_") || sym == "brasa_start" {
        return Some(("(runtime)".into(), true));
    }
    None
}

/// `--explain-link`: the bytes of code and data each package contributes to the program, from the
/// positions of the symbols in the assembled image (the same assembly the executable is made of).
pub fn link_report(file: &File, options: &CompileOptions, target: Target) -> Result<String, String> {
    let mut c = Compiler::new(target);
    c.opt = options.optimize;
    c.test_mode = options.test_mode;
    c.load(file).map_err(crate::comptime::map_error)?;
    c.declare().map_err(crate::comptime::map_error)?;
    let asm = c.generate().map_err(crate::comptime::map_error)?;
    let program = c.modules.first().map(|m| m.name.clone()).unwrap_or_default();
    let modules: Vec<(String, String)> = c.modules.iter().map(|m| (m.name.clone(), sym_of(&m.name))).collect();
    let (text, org) = match target {
        Target::Linux => (asm.clone(), LINUX_BASE + ELF_HEADERS),
        _ => (asm.replacen("\n.data\n", "\nrt_text_end:\nalign 4096\nrt_data_start:\n", 1), crate::pe::PE_BASE + 0x1000),
    };
    let assembled = crate::asm_x86::assemble(&text, org, 64)?;
    let end = org + assembled.bytes.len() as u64;
    let mut syms: Vec<(&String, u64)> = assembled.symbols.iter().map(|(k, v)| (k, *v)).filter(|(_, v)| *v >= org && *v <= end).collect();
    syms.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(b.0)));
    // bytes between one symbol and the next belong to the owner of the last symbol that has one
    let mut sizes: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut owner: Option<(String, bool)> = None;
    for (i, (name, at)) in syms.iter().enumerate() {
        if let Some(o) = symbol_owner(name, &modules) {
            owner = Some(o);
        }
        let next = syms.get(i + 1).map(|s| s.1).unwrap_or(end);
        if let Some((o, is_code)) = &owner {
            if matches!(name.as_str(), "rt_text_end" | "rt_data_start") {
                continue;
            }
            let e = sizes.entry(o.clone()).or_insert((0, 0));
            if *is_code {
                e.0 += next - at;
            } else {
                e.1 += next - at;
            }
        }
    }
    // what each package's reachable code can do: a function counts when its symbol is in the
    // program, an `extern` when its import slot is; generic code is always counted
    let effects_of = |m: &Module| -> crate::effects::Effects {
        let ms = sym_of(&m.name);
        let keep = |d: &Decl| -> bool {
            match &d.kind {
                DeclKind::Func(f) => {
                    if !f.generics.is_empty() {
                        return true;
                    }
                    let name = match &f.receiver {
                        Some(r) => match &r.ty {
                            Type::Named { path, args } if args.is_empty() => format!("B_{}_{}_{}", ms, path.last().cloned().unwrap_or_default(), f.name),
                            _ => return true,
                        },
                        None => format!("B_{}_{}", ms, f.name),
                    };
                    assembled.symbols.contains_key(&name)
                }
                DeclKind::Extern(e) => assembled.symbols.contains_key(&format!("GOT_{}", e.name)),
                _ => false,
            }
        };
        crate::effects::of_decls(&m.file.decls, &keep)
    };
    let effects: BTreeMap<String, String> = c.modules.iter().map(|m| (m.name.clone(), effects_of(m).show())).collect();
    let mut rows: Vec<(String, &str, u64, u64)> = sizes
        .into_iter()
        .map(|(k, (code, data))| {
            let kind = if k.starts_with('(') {
                "-"
            } else if k == program {
                "program"
            } else if std_source(&k).is_some() {
                "std"
            } else {
                "package"
            };
            (k, kind, code, data)
        })
        .collect();
    rows.sort_by(|a, b| (b.2 + b.3).cmp(&(a.2 + a.3)).then(a.0.cmp(&b.0)));
    let width = rows.iter().map(|r| r.0.len()).max().unwrap_or(7).max(7);
    let mut out = format!("{:<w$}  {:<8} {:>8} {:>8}  {}\n", "package", "kind", "code", "data", "effects", w = width);
    let (mut tc, mut td) = (0, 0);
    for (k, kind, code, data) in &rows {
        out.push_str(&format!("{:<w$}  {:<8} {:>8} {:>8}  {}\n", k, kind, code, data, effects.get(k).map(|s| s.as_str()).unwrap_or(""), w = width));
        tc += code;
        td += data;
    }
    out.push_str(&format!("{:<w$}  {:<8} {:>8} {:>8}\n", "total", "", tc, td, w = width));
    out.push_str(&format!("{} bytes of code and data\n", tc + td));
    Ok(out)
}

/// Compiles to a native Windows x86-64 executable. The compiler writes the PE file itself
/// (`pe::pe_image`): no assembler, linker or Visual Studio is involved.
pub fn compile_to_native(file: &File, output_exe: &str) -> Result<(), String> {
    compile_to_native_with(file, output_exe, &CompileOptions::default())
}

/// `compile_to_native` with explicit options. Nothing is written unless the image is complete.
pub fn compile_to_native_with(file: &File, output_exe: &str, options: &CompileOptions) -> Result<(), String> {
    let mut c = Compiler::new(Target::Windows);
    c.opt = options.optimize;
    c.test_mode = options.test_mode;
    c.load(file).map_err(crate::comptime::map_error)?;
    c.declare().map_err(crate::comptime::map_error)?;
    let asm = c.generate().map_err(crate::comptime::map_error)?;
    let image = crate::pe::pe_image(&asm, &c.win_imports, c.uses_ui)?;
    std::fs::write(output_exe, &image).map_err(|e| format!("cannot write {}: {}", output_exe, e))
}

/// The reference-count and live-object instructions of the runtime, made atomic (`lock` prefix)
/// for programs that use `shared` or `spawn_thread`.
pub(super) fn lock_counts(asm: &str) -> String {
    let mut out = String::with_capacity(asm.len() + 1024);
    for line in asm.lines() {
        let t = line.trim_start();
        let atomic = matches!(t, "inc qword ptr [rcx - 16]" | "dec qword ptr [rcx - 16]" | "inc qword ptr rt_live" | "dec qword ptr rt_live");
        if atomic {
            out.push_str("    lock ");
            out.push_str(t);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

