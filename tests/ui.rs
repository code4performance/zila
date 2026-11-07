//! The `ui` library on both targets. The same programs are built for Windows and for Linux and
//! driven with scripted input (`BRASA_UI_SCRIPT`), and must print the same text. Windows runs
//! need the Visual Studio build tools; Linux runs need WSL with an X display (WSLg). Each part is
//! skipped when its environment is missing.

use brasa::codegen_x64::{compile_to_linux, compile_to_native, CompileOptions};
use brasa::parse_source;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn temp(ext: &str) -> PathBuf {
    std::env::temp_dir().join(format!("brasa-ui-test-{}-{}.{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst), ext))
}

fn run_windows(src: &str, script: &str) -> Option<String> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = temp("exe");
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Ok(()) => {}
        Err(e) if e.contains("could not find Visual Studio") || e.contains("vcvars64") => {
            eprintln!("no native toolchain: skipping");
            return None;
        }
        Err(e) => panic!("compile error: {}", e),
    }
    let steps = temp("txt");
    std::fs::write(&steps, script).unwrap();
    let out = Command::new(&exe).env("BRASA_UI_SCRIPT", &steps).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    let _ = std::fs::remove_file(&steps);
    Some(String::from_utf8_lossy(&out.stdout).replace('\r', "") + &String::from_utf8_lossy(&out.stderr).replace('\r', ""))
}

fn wsl_path(p: &std::path::Path) -> String {
    let s = p.to_str().unwrap().replace('\\', "/");
    let (drive, rest) = s.split_at(2);
    format!("/mnt/{}{}", drive[..1].to_lowercase(), rest)
}

fn wsl_has_display() -> bool {
    Command::new("wsl.exe")
        .args(["-e", "sh", "-c", "test -S /tmp/.X11-unix/X0"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Builds for Linux and runs under WSL against the X server of WSLg.
fn run_linux(src: &str, script: &str, display: &str) -> Option<String> {
    if !wsl_has_display() {
        eprintln!("WSL with an X display is not available: skipping");
        return None;
    }
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = temp("bin");
    compile_to_linux(&file, exe.to_str().unwrap(), &CompileOptions::default()).unwrap_or_else(|e| panic!("compile error: {}", e));
    let steps = temp("txt");
    std::fs::write(&steps, script).unwrap();
    let name = exe.file_name().unwrap().to_str().unwrap().to_string();
    let cmd = format!(
        "cp '{}' /tmp/{n} && chmod +x /tmp/{n} && cp '{}' /tmp/{n}.txt && {} BRASA_UI_SCRIPT=/tmp/{n}.txt /tmp/{n} 2>&1; echo \"__EXIT:$?\"; rm -f /tmp/{n} /tmp/{n}.txt",
        wsl_path(&exe),
        wsl_path(&steps),
        display,
        n = name
    );
    let out = Command::new("wsl.exe").args(["-e", "sh", "-c", &cmd]).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    let _ = std::fs::remove_file(&steps);
    Some(String::from_utf8_lossy(&out.stdout).replace('\r', "").replace("__EXIT:0\n", ""))
}

/// Runs a program on every available target and checks that all print `want`.
fn same_on_both(src: &str, script: &str, want: &str) {
    let mut ran = 0;
    if let Some(out) = run_windows(src, script) {
        assert_eq!(out, want, "Windows output");
        ran += 1;
    }
    if let Some(out) = run_linux(src, script, "DISPLAY=:0") {
        assert_eq!(out, want, "Linux output");
        ran += 1;
    }
    eprintln!("ran on {} target(s)", ran);
}

const CUSTOMERS: &str = r#"package main

use ui

func main()
    var win = ui.Window("T", 400, 300)
    var name = ui.TextBox()
    var list = ui.ListView()
    var add = ui.Button("Add")
    name.onChange(func()
        println("change [{name.text}]")
    end)
    add.onClick(func()
        list.add(name.text)
        println("added {name.text}, count {list.count}")
        name.text = ""
    end)
    list.onSelect(func()
        println("selected {list.selected}")
    end)
    win.layout(ui.vstack(name, add, list))
    win.show()
    ui.run()
    println("closed")
end
"#;

#[test]
fn add_to_a_list_select_and_close() {
    same_on_both(
        CUSTOMERS,
        "type Ana\nclick 0\ntype Bo\nkey backspace\nclick 0\nselect 1\nkey up\nclose\n",
        "change [A]\nchange [An]\nchange [Ana]\nadded Ana, count 1\nchange []\nchange [B]\nchange [Bo]\nchange [B]\nadded B, count 2\nchange []\nselected 1\nselected 0\nclosed\n",
    );
}

#[test]
fn setting_the_same_text_is_not_a_change() {
    let src = r#"package main

use ui

func main()
    var win = ui.Window("T")
    var box = ui.TextBox("a")
    box.onChange(func()
        println("change [{box.text}]")
    end)
    win.layout(ui.vstack(box))
    box.text = "a"
    box.text = "b"
    box.text = "b"
    println("text {box.text}")
    win.show()
    ui.run()
end
"#;
    same_on_both(src, "close\n", "change [b]\ntext b\n");
}

#[test]
fn enter_clicks_the_first_button_and_the_loop_ends_without_a_script_close() {
    let src = r#"package main

use ui

func main()
    var win = ui.Window("T")
    var box = ui.TextBox()
    var ok = ui.Button("OK")
    ok.onClick(func()
        println("ok with {box.text}")
    end)
    win.layout(ui.vstack(box, ok))
    win.show()
    ui.run()
    println("end")
end
"#;
    same_on_both(src, "type hi\nkey enter\n", "ok with hi\nend\n");
}

#[test]
fn the_list_and_the_labels_report_their_state() {
    let src = r#"package main

use ui

func main()
    var win = ui.Window("T")
    var list = ui.ListView()
    list.add("one")
    list.add("two")
    list.add("three")
    list.remove(1)
    println("count {list.count}, selected {list.selected}")
    list.clear()
    println("count {list.count}")
    var label = ui.Label("hello")
    label.text = "bye"
    println(label.text)
    win.layout(ui.vstack(label, list))
    win.show()
    ui.run()
end
"#;
    same_on_both(src, "close\n", "count 2, selected -1\ncount 0\nbye\n");
}

// ---------------------------------------------------------------- the two layers are one API

fn shared_region(path: &str) -> String {
    let text = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
    let a = text.find("// ---- shared begin ----").unwrap_or_else(|| panic!("{} has no shared region", path));
    let b = text.find("// ---- shared end ----").unwrap();
    text[a..b].to_string()
}

#[test]
fn the_shared_region_is_the_same_text_in_both_layers() {
    let windows = shared_region("std/ui.br");
    let linux = shared_region("std/ui.linux.br");
    assert_eq!(windows, linux, "the shared region of std/ui.br and std/ui.linux.br differ");
    assert!(windows.contains("func Window(") && windows.contains("func alert(") && windows.contains("func run()"));
    // nothing platform-shaped in the public surface
    assert!(!windows.contains("hwnd") && !windows.contains("HWND"));
}

/// Every `be_*` function the shared region calls exists, with the same signature, in both layers.
#[test]
fn both_layers_implement_the_same_backend_functions() {
    let sig = |text: &str, name: &str| -> Option<String> {
        let start = text.find(&format!("func {}(", name))?;
        Some(text[start..].lines().next().unwrap().trim().to_string())
    };
    let windows = std::fs::read_to_string("std/ui.br").unwrap().replace("\r\n", "\n");
    let linux = std::fs::read_to_string("std/ui.linux.br").unwrap().replace("\r\n", "\n");
    let shared = shared_region("std/ui.br");
    let mut names: Vec<String> = Vec::new();
    for word in shared.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        if word.starts_with("be_") && word.len() > 3 && !names.contains(&word.to_string()) {
            names.push(word.to_string());
        }
    }
    assert!(names.len() > 20, "found {:?}", names);
    for n in &names {
        let w = sig(&windows, n).unwrap_or_else(|| panic!("std/ui.br does not define {}", n));
        let l = sig(&linux, n).unwrap_or_else(|| panic!("std/ui.linux.br does not define {}", n));
        assert_eq!(w, l, "{} differs between the layers", n);
    }
}

// ---------------------------------------------------------------- the X11 layer on a real server

/// Brasa helpers shared by the X-level test programs below.
const X_HELPERS: &str = r#"
func send(window: int, ev: sys.Buf)
    let r = ui.req(25, 0, 11)
    ui.put32(r + 4, window)
    ui.put32(r + 8, 0)
    for i in 0..32
        sys.put8(r + 12 + i, sys.get8(int(ev.ptr) + i))
    end
    ui.flush_x()
end

func keycode_of(sym: int) -> int
    for code in ui.min_keycode..ui.min_keycode + 240
        if ui.get32(int(ui.keysyms.ptr) + (code - ui.min_keycode) * ui.keysyms_per * 4) == sym
            return code
        end
    end
    return 0
end

func key(window: int, sym: int, state: int)
    let ev = sys.buffer(32)
    sys.put8(int(ev.ptr), 2)
    sys.put8(int(ev.ptr) + 1, keycode_of(sym))
    ui.put32(int(ev.ptr) + 12, window)
    ui.put16(int(ev.ptr) + 28, state)
    send(window, ev)
end

func mouse(window: int, kind: int, button: int, x: int, y: int)
    let ev = sys.buffer(32)
    sys.put8(int(ev.ptr), kind)
    sys.put8(int(ev.ptr) + 1, button)
    ui.put32(int(ev.ptr) + 12, window)
    ui.put16(int(ev.ptr) + 24, x)
    ui.put16(int(ev.ptr) + 26, y)
    send(window, ev)
end

func click(window: int, x: int, y: int)
    mouse(window, 4, 1, x, y)
    mouse(window, 5, 1, x, y)
end
"#;

fn x_program(body: &str) -> String {
    format!("package main\n\nuse ui\nuse c\nuse sys\n{}\n{}", X_HELPERS, body)
}

#[test]
fn a_window_has_the_requested_size_and_title_on_the_server() {
    let src = x_program(
        r#"
func geometry(id: int) -> []int
    let r = ui.req(14, 0, 2)
    ui.put32(r + 4, id)
    ui.flush_x()
    let reply = sys.buffer(32)
    ui.read_full(int(reply.ptr), 32)
    var v: []int
    v.add(ui.get16(int(reply.ptr) + 16))
    v.add(ui.get16(int(reply.ptr) + 18))
    return v
end

func title_of(id: int) -> text
    let r = ui.req(20, 0, 6)
    ui.put32(r + 4, id)
    ui.put32(r + 8, 39)
    ui.put32(r + 12, 31)
    ui.put32(r + 16, 0)
    ui.put32(r + 20, 100)
    ui.flush_x()
    let head = sys.buffer(32)
    ui.read_full(int(head.ptr), 32)
    let words = ui.get32(int(head.ptr) + 4)
    let len = ui.get32(int(head.ptr) + 16)
    let body = sys.buffer(words * 4 + 8)
    ui.read_full(int(body.ptr), words * 4)
    return c.from_bytes(body.ptr, len)
end

func main()
    let win = ui.Window("Hello X", 400, 300)
    let g = geometry(int(win.id))
    let dpi = ui.be_dpi(win.id)
    println("size {g[0] * 96 / dpi}x{g[1] * 96 / dpi} title {title_of(int(win.id))}")
end
"#,
    );
    let Some(out) = run_linux(&src, "", "DISPLAY=:0") else { return };
    assert_eq!(out, "size 400x300 title Hello X\n");
}

#[test]
fn controls_are_drawn_where_the_layout_puts_them() {
    let src = x_program(
        r#"
func image(id: int, w: int, h: int) -> sys.Buf
    let r = ui.req(73, 2, 5)
    ui.put32(r + 4, id)
    ui.put16(r + 8, 0)
    ui.put16(r + 10, 0)
    ui.put16(r + 12, w)
    ui.put16(r + 14, h)
    ui.put32(r + 16, 0xFFFFFFFF)
    ui.flush_x()
    let head = sys.buffer(32)
    ui.read_full(int(head.ptr), 32)
    let words = ui.get32(int(head.ptr) + 4)
    let data = sys.buffer(words * 4 + 8)
    ui.read_full(int(data.ptr), words * 4)
    return data
end

func pixel(data: sys.Buf, w: int, x: int, y: int) -> int
    return ui.get32(int(data.ptr) + (y * w + x) * 4) bitand 0xFFFFFF
end

func main()
    let win = ui.Window("Pixels", 400, 300)
    let box = ui.TextBox("abc")
    let ok = ui.Button("OK")
    let list = ui.ListView()
    list.add("one")
    win.layout(ui.vstack(box, ok, list))
    win.show()
    ui.paint_dirty()
    let data = image(ui.wins[ui.win_index(int(win.id))].pixmap, 400, 300)
    println("background {pixel(data, 400, 2, 2) == ui.C_BG}")
    println("focused border {pixel(data, 400, 12, 12) == ui.C_FOCUS}")
    println("field {pixel(data, 400, 380, 20) == ui.C_FIELD}")
    println("button {pixel(data, 400, 20, 52) == ui.C_FACE}")
    println("list {pixel(data, 400, 380, 100) == ui.C_FIELD}")
end
"#,
    );
    let Some(out) = run_linux(&src, "", "DISPLAY=:0") else { return };
    assert_eq!(out, "background true\nfocused border true\nfield true\nbutton true\nlist true\n");
}

#[test]
fn real_key_and_mouse_events_from_the_server_reach_the_handlers() {
    let src = x_program(
        r#"
func main()
    let win = ui.Window("Events", 400, 300)
    let box = ui.TextBox()
    let ok = ui.Button("OK")
    let list = ui.ListView()
    list.add("one")
    list.add("two")
    list.add("three")
    box.onChange(func()
        println("change [{box.text}]")
    end)
    ok.onClick(func()
        println("click, box [{box.text}]")
    end)
    list.onSelect(func()
        println("select {list.selected}")
    end)
    win.layout(ui.vstack(box, ok, list))
    win.show()
    ui.paint_dirty()
    let id = int(win.id)
    let b = ui.widgets[ui.widget_index(int(ok.id))]
    let l = ui.widgets[ui.widget_index(int(list.id))]
    key(id, 104, 1)
    key(id, 105, 0)
    key(id, 0xFF08, 0)
    key(id, 105, 0)
    key(id, 0xFF51, 0)
    key(id, 120, 0)
    key(id, 0xFF0D, 0)
    click(id, b.x + 10, b.y + 10)
    click(id, l.x + 10, l.y + 2 + ui.ROW + 4)
    key(id, 0xFF54, 0)
    key(id, 0xFF52, 0)
    key(id, 0xFF52, 0)
    let cfg = sys.buffer(32)
    sys.put8(int(cfg.ptr), 22)
    ui.put32(int(cfg.ptr) + 4, id)
    ui.put32(int(cfg.ptr) + 8, id)
    ui.put16(int(cfg.ptr) + 20, 400)
    ui.put16(int(cfg.ptr) + 22, 400)
    send(id, cfg)
    let msg = sys.buffer(32)
    sys.put8(int(msg.ptr), 33)
    sys.put8(int(msg.ptr) + 1, 32)
    ui.put32(int(msg.ptr) + 4, id)
    ui.put32(int(msg.ptr) + 8, ui.atom_protocols)
    ui.put32(int(msg.ptr) + 12, ui.atom_delete)
    send(id, msg)
    ui.run()
    let nb = ui.widgets[ui.widget_index(int(box.id))]
    let nl = ui.widgets[ui.widget_index(int(list.id))]
    println("heights box {nb.h} list {nl.h} (was {l.h}), selected {list.selected}")
end
"#,
    );
    let Some(out) = run_linux(&src, "", "DISPLAY=:0") else { return };
    assert_eq!(
        out,
        "change [H]\nchange [Hi]\nchange [H]\nchange [Hi]\nchange [Hxi]\nclick, box [Hxi]\nclick, box [Hxi]\nselect 1\nselect 2\nselect 1\nselect 0\nheights box 28 list 300 (was 200), selected 0\n"
    );
}

#[test]
fn enter_and_escape_dismiss_an_alert() {
    let src = x_program(
        r#"
func main()
    ui.ensure_connected()
    let win = ui.Window("Alert", 240, 120)
    let ok = ui.Button("OK")
    ok.onClick(func()
        win.close()
    end)
    win.layout(ui.vstack(ok))
    ui.set_alert_window(int(win.id))
    win.show()
    ui.paint_dirty()
    key(int(win.id), 0xFF1B, 0)
    ui.event_loop(int(win.id))
    println("escape closed it: {ui.wins[ui.win_index(int(win.id))].open == false}")
    let second = ui.Window("Alert 2", 240, 120)
    let ok2 = ui.Button("OK")
    ok2.onClick(func()
        second.close()
    end)
    second.layout(ui.vstack(ok2))
    second.show()
    ui.paint_dirty()
    key(int(second.id), 0xFF0D, 0)
    ui.event_loop(int(second.id))
    println("enter closed it: {ui.wins[ui.win_index(int(second.id))].open == false}")
    ui.alert("never shown while scripted")
    println("done")
end
"#,
    );
    // the script file only has to exist: a scripted run does not show alerts
    let Some(out) = run_linux(&src, "close\n", "DISPLAY=:0") else { return };
    assert_eq!(out, "escape closed it: true\nenter closed it: true\ndone\n");
}

#[test]
fn without_a_display_the_program_stops_with_one_clear_error() {
    let src = "package main\n\nuse ui\n\nfunc main()\n    let win = ui.Window(\"T\")\n    win.show()\n    ui.run()\nend\n";
    let Some(out) = run_linux(src, "", "unset DISPLAY;") else { return };
    assert!(out.starts_with("error: ui: cannot open a display: DISPLAY is not set"), "{}", out);
    assert!(out.contains("__EXIT:1"), "{}", out);
    let Some(out) = run_linux(src, "", "DISPLAY=:77") else { return };
    assert!(out.starts_with("error: ui: cannot open a display: cannot connect to /tmp/.X11-unix/X77"), "{}", out);
    let Some(out) = run_linux(src, "", "DISPLAY=far.away:0") else { return };
    assert!(out.contains("only local displays are supported"), "{}", out);
}
