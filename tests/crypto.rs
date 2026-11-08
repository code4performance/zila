//! The `crypto` library (spec `crypto-library`): hashes and HMAC against the published test
//! vectors, PBKDF2, encodings, constant-time comparison and random bytes. Programs are compiled
//! to native executables and run; the Linux half (under WSL) is skipped without WSL.

use brasa::codegen_x64::{compile_to_linux, CompileOptions};
use brasa::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;

static N: AtomicUsize = AtomicUsize::new(0);
static INIT: Once = Once::new();

fn setup() {
    INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("brasa-crypto-test-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        brasa::comptime::set_cache_dir(&dir);
    });
}

fn program(body: &str) -> String {
    format!("package main\n\nuse crypto\nuse text\n\nfunc main()\n{}\nend\n", body)
}

fn build(src: &str) -> Result<std::path::PathBuf, String> {
    setup();
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("brasa-crypto-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    compile_to_native(&file, exe.to_str().unwrap()).map(|_| exe)
}

fn run(src: &str) -> String {
    let exe = build(src).unwrap_or_else(|e| panic!("compile error: {}", e));
    let out = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    String::from_utf8_lossy(&out.stdout).replace('\r', "") + &String::from_utf8_lossy(&out.stderr).replace('\r', "")
}

fn kernel_error(src: &str) -> String {
    setup();
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    brasa::codegen_x64::generate_asm_for(&file, &brasa::codegen_x64::CompileOptions::default(), brasa::codegen_x64::Target::Bare)
        .expect_err("expected the kernel build to fail")
}

fn wsl_ok() -> bool {
    Command::new("wsl.exe").args(["-e", "sh", "-c", "echo ready"]).output().map(|o| String::from_utf8_lossy(&o.stdout).contains("ready")).unwrap_or(false)
}

/// Runs the program on Linux under WSL; `None` without WSL.
fn run_linux(src: &str) -> Option<String> {
    if !wsl_ok() {
        eprintln!("WSL is not available: skipping the Linux half");
        return None;
    }
    let file = parse_source(src).unwrap();
    let exe = std::env::temp_dir().join(format!("brasa-crypto-linux-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    setup();
    compile_to_linux(&file, exe.to_str().unwrap(), &CompileOptions::default()).unwrap_or_else(|e| panic!("compile error: {}", e));
    let p = exe.to_str().unwrap().replace('\\', "/");
    let wsl = format!("/mnt/{}{}", p[..1].to_ascii_lowercase(), &p[2..]);
    let name = exe.file_name().unwrap().to_str().unwrap().to_string();
    let script = format!("cp '{}' /tmp/{n} && chmod +x /tmp/{n} && /tmp/{n}; rm -f /tmp/{n}", wsl, n = name);
    let out = Command::new("wsl.exe").args(["-e", "sh", "-c", &script]).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    Some(String::from_utf8_lossy(&out.stdout).replace('\r', ""))
}

const HASH_BODY: &str = r#"
print(crypto.hex(crypto.sha256("")))
print(crypto.hex(crypto.sha256("abc")))
print(crypto.hex(crypto.sha256("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")))
var st = crypto.sha256_new()
let big = text.repeat("a", 1000000)
var at = 0
while at < big.len
    var to = at + 7
    if to > big.len
        to = big.len
    end
    st.add(big.slice(at, to))
    at = to
end
print(crypto.hex(st.finish()))
print(crypto.hex(crypto.sha256(big)))
print(crypto.hex(crypto.md5("")))
print(crypto.hex(crypto.md5("abc")))
print(crypto.hex(crypto.md5("message digest")))
print(crypto.hex(crypto.md5("12345678901234567890123456789012345678901234567890123456789012345678901234567890")))
"#;

const HASH_EXPECTED: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1
cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0
cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0
d41d8cd98f00b204e9800998ecf8427e
900150983cd24fb0d6963f7d28e17f72
f96b697d7cb7938d525a2f31aaf161d0
57edf4a22be3c955ac49da2e2107b67a
";

#[test]
fn sha256_and_md5_match_the_published_vectors_and_streaming_equals_one_shot() {
    let src = program(HASH_BODY);
    assert_eq!(run(&src), HASH_EXPECTED);
    if let Some(linux) = run_linux(&src) {
        assert_eq!(linux, HASH_EXPECTED);
    }
}

const MAC_BODY: &str = r#"
var key1: []long = []
for i in 0..20
    key1.add(11)
end
print(crypto.hex(crypto.hmac_sha256(crypto.from_bytes(key1), "Hi There")))
print(crypto.hex(crypto.hmac_sha256("Jefe", "what do ya want for nothing?")))
var key6: []long = []
for i in 0..131
    key6.add(170)
end
print(crypto.hex(crypto.hmac_sha256(crypto.from_bytes(key6), "Test Using Larger Than Block-Size Key - Hash Key First")))
match crypto.pbkdf2_sha256("passwd", "salt", 1, 64)
    ok(k) -> print(crypto.hex(k))
    err(e) -> print(e.message)
end
match crypto.pbkdf2_sha256("password", "salt", 2, 32)
    ok(k) -> print(crypto.hex(k))
    err(e) -> print(e.message)
end
match crypto.pbkdf2_sha256("password", "salt", 4096, 32)
    ok(k) -> print(crypto.hex(k))
    err(e) -> print(e.message)
end
match crypto.pbkdf2_sha256("p", "s", 0, 32)
    ok(k) -> print("accepted")
    err(e) -> print(e.message)
end
"#;

const MAC_EXPECTED: &str = "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7
5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843
60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54
55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc49ca9cccf179b645991664b39d77ef317c71b845b1e30bd509112041d3a19783
ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43
c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a
pbkdf2: iterations and length must be positive
";

#[test]
fn hmac_and_pbkdf2_match_the_published_vectors() {
    let src = program(MAC_BODY);
    assert_eq!(run(&src), MAC_EXPECTED);
    if let Some(linux) = run_linux(&src) {
        assert_eq!(linux, MAC_EXPECTED);
    }
}

const ENC_BODY: &str = r#"
for s in ["", "f", "fo", "foo", "foob", "fooba", "foobar"]
    print("[{crypto.base64_encode(s)}]")
end
var all: []long = []
for i in 0..256
    all.add(long(i))
end
let bin = crypto.from_bytes(all)
print(bin.len)
match crypto.base64_decode(crypto.base64_encode(bin))
    ok(b) -> print(b.len, crypto.hex(b) == crypto.hex(bin))
    err(e) -> print(e.message)
end
match crypto.base64url_decode(crypto.base64url_encode(bin))
    ok(b) -> print(b.len, crypto.hex(b) == crypto.hex(bin))
    err(e) -> print(e.message)
end
print(crypto.base64url_encode(crypto.from_bytes([251, 255, 254])))
for s in ["abc", "ab!d", "Zg=", "Zh=="]
    match crypto.base64_decode(s)
        ok(b) -> print("accepted {s}")
        err(e) -> print(e.message)
    end
end
match crypto.unhex("00ff10Ab")
    ok(b) -> print(crypto.hex(b))
    err(e) -> print(e.message)
end
for s in ["abc", "zz"]
    match crypto.unhex(s)
        ok(b) -> print("accepted")
        err(e) -> print(e.message)
    end
end
print(crypto.equal("abc", "abc"), crypto.equal("abc", "abd"), crypto.equal("abc", "ab"))
"#;

const ENC_EXPECTED: &str = "[]
[Zg==]
[Zm8=]
[Zm9v]
[Zm9vYg==]
[Zm9vYmE=]
[Zm9vYmFy]
256
256 true
256 true
-__-
base64: bad character at byte 0
base64: bad character at byte 2
base64: length is not a multiple of 4
base64: bad trailing bits
00ff10ab
hex: odd number of digits
hex: bad digit at byte 0
true false false
";

#[test]
fn base64_hex_and_equal_follow_the_spec() {
    let src = program(ENC_BODY);
    // `abc` is rejected for its length (3 is not a multiple of 4)
    let expected = ENC_EXPECTED.replacen("base64: bad character at byte 0\n", "base64: length is not a multiple of 4\n", 1);
    assert_eq!(run(&src), expected);
    if let Some(linux) = run_linux(&src) {
        assert_eq!(linux, expected);
    }
}

#[test]
fn random_bytes_have_the_asked_size_and_differ() {
    let body = r#"
let a = crypto.random(32) ?? "x"
let b = crypto.random(32) ?? "x"
print(a.len, b.len, crypto.equal(a, b))
print((crypto.random(0) ?? "x").len)
match crypto.random(-1)
    ok(v) -> print("accepted")
    err(e) -> print(e.message)
end
"#;
    let src = program(body);
    let expected = "32 32 false\n0\nrandom: negative length\n";
    assert_eq!(run(&src), expected);
    if let Some(linux) = run_linux(&src) {
        assert_eq!(linux, expected);
    }
}

#[test]
fn the_kernel_build_rejects_crypto_naming_the_library() {
    let src = "package main\n\nuse crypto\n\nfunc main()\n    print(crypto.hex(\"a\"))\nend\n";
    let e = kernel_error(src);
    assert!(e.contains("crypto") || e.contains("sys"), "{}", e);
}
