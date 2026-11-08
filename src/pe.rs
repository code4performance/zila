//! The Windows PE32+ executable writer: no `ml64`, no `link`, no Visual Studio.
//!
//! The built-in assembler turns the generated assembly into bytes (code first, then data, with
//! the data starting on a page); this module writes the headers, the import directory and the
//! manifest resource around them. Sections: `.text` (code, read and execute), `.data` (data and
//! the import address table, read and write), `.idata` (import directory) and `.rsrc` (the
//! manifest), with 4 KiB virtual and 512-byte file alignment. The image has a fixed base and no
//! relocations.
//!
//! The assembly reserves one `GOT_<name> dq 0` slot per imported function, grouped by library
//! with a zero slot after each group; those slots are the import address table (the loader
//! writes the function addresses into them, guided by the lookup tables built here).

/// Where a Windows executable is loaded (fixed: the image has no relocations).
pub const PE_BASE: u64 = 0x140000000;
const SECTION_ALIGN: u64 = 4096;
const FILE_ALIGN: u64 = 512;

/// Common controls 6, per-monitor DPI awareness and the UTF-8 code page.
const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
    </windowsSettings>
  </application>
</assembly>
"#;

/// The DLL that provides a library named in `#[link]`: the name itself, except for the C
/// runtime, whose import library `ucrt` stands for `ucrtbase.dll`.
fn dll_file(lib: &str) -> &str {
    match lib {
        "ucrt" => "ucrtbase",
        other => other,
    }
}

fn align_up(n: u64, a: u64) -> u64 {
    (n + a - 1) / a * a
}

/// The text of an assembler error: at the Zila Lang line of an inline `asm` block when the failing
/// line belongs to one (the code generator leaves a `; asm file.br:line` marker before each block).
fn asm_error(asm: &str, e: &str) -> String {
    let Some(rest) = e.strip_prefix("asm line ") else { return e.to_string() };
    let Some((n, msg)) = rest.split_once(": ") else { return e.to_string() };
    let Ok(n) = n.parse::<usize>() else { return e.to_string() };
    let lines: Vec<&str> = asm.lines().collect();
    for i in (0..n.min(lines.len())).rev() {
        let l = lines[i].trim();
        if let Some(pos) = l.strip_prefix("; asm ") {
            return format!("{}: inline asm: {}", pos, msg);
        }
        if l.ends_with(" proc") || l.ends_with(" endp") {
            break;
        }
    }
    format!("internal error: {} (generated assembly line {})", msg, n)
}

/// The executable for the generated assembly. `imports` lists `(library, functions)` in the order
/// the slots appear in the assembly; `gui` selects the Windows subsystem instead of the console.
pub fn pe_image(asm: &str, imports: &[(String, Vec<String>)], gui: bool) -> Result<Vec<u8>, String> {
    const MARK: &str = "\n.data\n";
    let Some(at) = asm.find(MARK) else {
        return Err("internal error: no data section".into());
    };
    // the data starts on a page: the two lines added here shift the lines after the code
    let data_line = asm[..at].lines().count() + 3;
    let text = asm.replacen(MARK, "\nrt_text_end:\nalign 4096\nrt_data_start:\n", 1);
    let text_va = 0x1000u64;
    let assembled = crate::asm_x86::assemble(&text, PE_BASE + text_va, 64).map_err(|e| {
        let shifted = match e.strip_prefix("asm line ").and_then(|r| r.split_once(": ")) {
            Some((n, rest)) => match n.parse::<usize>() {
                Ok(n) if n > data_line => format!("asm line {}: {}", n - 2, rest),
                _ => e.clone(),
            },
            None => e.clone(),
        };
        asm_error(asm, &shifted)
    })?;
    let sym = |n: &str| assembled.symbols.get(n).copied().ok_or_else(|| format!("internal error: no symbol {}", n));
    let entry = sym("zila_start")? - PE_BASE;
    let text_len = sym("rt_text_end")? - (PE_BASE + text_va);
    let data_start = (sym("rt_data_start")? - (PE_BASE + text_va)) as usize;
    let data_va = sym("rt_data_start")? - PE_BASE;
    let code = &assembled.bytes[..text_len as usize];
    let mut data = assembled.bytes[data_start..].to_vec();

    // layout of the sections
    let text_raw = align_up(text_len, FILE_ALIGN);
    let data_raw = align_up(data.len() as u64, FILE_ALIGN);
    let idata_va = data_va + align_up((data.len() as u64).max(1), SECTION_ALIGN);

    // the import directory: descriptors, lookup tables, hint/name entries, library names
    let n = imports.len();
    if n == 0 {
        return Err("internal error: no imports".into());
    }
    let desc_len = (n as u64 + 1) * 20;
    let mut ilt_at = Vec::new();
    let mut at = desc_len;
    for (_, syms) in imports {
        ilt_at.push(at);
        at += (syms.len() as u64 + 1) * 8;
    }
    let mut names: Vec<u8> = Vec::new();
    let mut hint_at: Vec<Vec<u64>> = Vec::new();
    for (_, syms) in imports {
        let mut v = Vec::new();
        for s in syms {
            v.push(at + names.len() as u64);
            names.extend_from_slice(&0u16.to_le_bytes());
            names.extend_from_slice(s.as_bytes());
            names.push(0);
            if names.len() % 2 == 1 {
                names.push(0);
            }
        }
        hint_at.push(v);
    }
    let mut dll_at = Vec::new();
    for (dll, _) in imports {
        dll_at.push(at + names.len() as u64);
        names.extend_from_slice(dll_file(dll).as_bytes());
        names.extend_from_slice(b".dll\0");
    }
    let rva = |o: u64| (idata_va + o) as u32;
    let mut first_slots = Vec::new();
    for (_, syms) in imports {
        first_slots.push(sym(&format!("GOT_{}", syms[0]))? - PE_BASE);
    }
    // the slots start out as the lookup table (the loader does not bind a table that starts empty)
    for i in 0..n {
        for (j, h) in hint_at[i].iter().enumerate() {
            let at = (first_slots[i] - data_va) as usize + 8 * j;
            data[at..at + 8].copy_from_slice(&(rva(*h) as u64).to_le_bytes());
        }
    }
    let mut idata: Vec<u8> = Vec::new();
    for i in 0..n {
        idata.extend_from_slice(&rva(ilt_at[i]).to_le_bytes()); // OriginalFirstThunk
        idata.extend_from_slice(&0u32.to_le_bytes());
        idata.extend_from_slice(&0u32.to_le_bytes());
        idata.extend_from_slice(&rva(dll_at[i]).to_le_bytes());
        idata.extend_from_slice(&(first_slots[i] as u32).to_le_bytes()); // FirstThunk: our slots
    }
    idata.extend_from_slice(&[0u8; 20]);
    for i in 0..n {
        for h in &hint_at[i] {
            idata.extend_from_slice(&(rva(*h) as u64).to_le_bytes());
        }
        idata.extend_from_slice(&0u64.to_le_bytes());
    }
    idata.extend_from_slice(&names);
    let iat_start = first_slots[0];
    let iat_end = first_slots[n - 1] + (imports[n - 1].1.len() as u64 + 1) * 8;
    let idata_raw = align_up(idata.len() as u64, FILE_ALIGN);
    let rsrc_va = idata_va + align_up(idata.len() as u64, SECTION_ALIGN);

    // the manifest as resource type 24 (RT_MANIFEST), name 1, language 1033
    let manifest = MANIFEST.as_bytes();
    let mut rsrc: Vec<u8> = Vec::new();
    let dir = |rsrc: &mut Vec<u8>, id: u32, sub: u32| {
        rsrc.extend_from_slice(&[0u8; 12]);
        rsrc.extend_from_slice(&0u16.to_le_bytes()); // no named entries
        rsrc.extend_from_slice(&1u16.to_le_bytes()); // one id entry
        rsrc.extend_from_slice(&id.to_le_bytes());
        rsrc.extend_from_slice(&(0x80000000 | sub).to_le_bytes());
    };
    dir(&mut rsrc, 24, 24);
    dir(&mut rsrc, 1, 48);
    dir(&mut rsrc, 1033, 0); // the last level points at the data entry (no high bit)
    let len = rsrc.len();
    rsrc[len - 4..].copy_from_slice(&72u32.to_le_bytes());
    rsrc.extend_from_slice(&((rsrc_va + 88) as u32).to_le_bytes());
    rsrc.extend_from_slice(&(manifest.len() as u32).to_le_bytes());
    rsrc.extend_from_slice(&0u32.to_le_bytes());
    rsrc.extend_from_slice(&0u32.to_le_bytes());
    rsrc.extend_from_slice(manifest);
    let rsrc_raw = align_up(rsrc.len() as u64, FILE_ALIGN);
    let image_size = rsrc_va + align_up(rsrc.len() as u64, SECTION_ALIGN);

    let text_off = FILE_ALIGN;
    let data_off = text_off + text_raw;
    let idata_off = data_off + data_raw;
    let rsrc_off = idata_off + idata_raw;

    let mut out: Vec<u8> = Vec::new();
    // DOS header: "MZ" and the offset of the PE header
    out.extend_from_slice(b"MZ");
    out.resize(0x3C, 0);
    out.extend_from_slice(&0x40u32.to_le_bytes());
    out.extend_from_slice(b"PE\0\0");
    // COFF header
    out.extend_from_slice(&0x8664u16.to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 12]);
    out.extend_from_slice(&240u16.to_le_bytes());
    out.extend_from_slice(&0x0023u16.to_le_bytes()); // executable, no relocations, large addresses
    // optional header (PE32+)
    out.extend_from_slice(&0x20Bu16.to_le_bytes());
    out.extend_from_slice(&[14, 0]);
    out.extend_from_slice(&(text_raw as u32).to_le_bytes());
    out.extend_from_slice(&((data_raw + idata_raw + rsrc_raw) as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(entry as u32).to_le_bytes());
    out.extend_from_slice(&(text_va as u32).to_le_bytes());
    out.extend_from_slice(&PE_BASE.to_le_bytes());
    out.extend_from_slice(&(SECTION_ALIGN as u32).to_le_bytes());
    out.extend_from_slice(&(FILE_ALIGN as u32).to_le_bytes());
    for v in [6u16, 0, 0, 0, 6, 0] {
        out.extend_from_slice(&v.to_le_bytes()); // OS, image and subsystem versions
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(image_size as u32).to_le_bytes());
    out.extend_from_slice(&(FILE_ALIGN as u32).to_le_bytes());
    let checksum_at = out.len();
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(if gui { 2u16 } else { 3u16 }).to_le_bytes());
    out.extend_from_slice(&0x8100u16.to_le_bytes()); // NX compatible, terminal server aware
    for v in [0x100000u64, 0x1000, 0x100000, 0x1000] {
        out.extend_from_slice(&v.to_le_bytes()); // stack and heap: reserve, commit
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&16u32.to_le_bytes());
    let mut dirs = [(0u32, 0u32); 16];
    dirs[1] = (idata_va as u32, desc_len as u32);
    dirs[2] = (rsrc_va as u32, rsrc.len() as u32);
    dirs[12] = (iat_start as u32, (iat_end - iat_start) as u32);
    for (a, b) in dirs {
        out.extend_from_slice(&a.to_le_bytes());
        out.extend_from_slice(&b.to_le_bytes());
    }
    // section table
    let section = |out: &mut Vec<u8>, name: &str, vsize: u64, va: u64, raw: u64, off: u64, flags: u32| {
        let mut nm = name.as_bytes().to_vec();
        nm.resize(8, 0);
        out.extend_from_slice(&nm);
        out.extend_from_slice(&(vsize as u32).to_le_bytes());
        out.extend_from_slice(&(va as u32).to_le_bytes());
        out.extend_from_slice(&(raw as u32).to_le_bytes());
        out.extend_from_slice(&(off as u32).to_le_bytes());
        out.extend_from_slice(&[0u8; 12]);
        out.extend_from_slice(&flags.to_le_bytes());
    };
    section(&mut out, ".text", text_len, text_va, text_raw, text_off, 0x60000020);
    section(&mut out, ".data", data.len() as u64, data_va, data_raw, data_off, 0xC0000040);
    section(&mut out, ".idata", idata.len() as u64, idata_va, idata_raw, idata_off, 0x40000040);
    section(&mut out, ".rsrc", rsrc.len() as u64, rsrc_va, rsrc_raw, rsrc_off, 0x40000040);
    out.resize(FILE_ALIGN as usize, 0);
    for (bytes, raw) in [(code, text_raw), (&data[..], data_raw), (&idata[..], idata_raw), (&rsrc[..], rsrc_raw)] {
        out.extend_from_slice(bytes);
        out.resize(out.len() + (raw as usize - bytes.len()), 0);
    }
    // the checksum: 16-bit words with the carries folded in, plus the file size
    let mut sum: u64 = 0;
    for (i, w) in out.chunks(2).enumerate() {
        if i * 2 == checksum_at || i * 2 == checksum_at + 2 {
            continue;
        }
        let v = if w.len() == 2 { u16::from_le_bytes([w[0], w[1]]) as u64 } else { w[0] as u64 };
        sum += v;
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    sum = (sum & 0xFFFF) + (sum >> 16);
    let checksum = (sum + out.len() as u64) as u32;
    out[checksum_at..checksum_at + 4].copy_from_slice(&checksum.to_le_bytes());
    Ok(out)
}
