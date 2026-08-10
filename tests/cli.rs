//! End-to-end tests: drive the real `ewg` binary in temp sandboxes.

use std::path::PathBuf;
use std::process::{Command, Output};

struct Sandbox {
    dir: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        Sandbox {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let p = self.path(name);
        std::fs::write(&p, contents).unwrap();
        p
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ewg"))
            .args(args)
            .current_dir(self.dir.path())
            // isolate registry so tests never touch ~/.config
            .env("EWG_REGISTRY", self.path("registry.toml"))
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "expected success for {args:?}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn fails(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(!out.status.success(), "expected failure for {args:?}");
        String::from_utf8(out.stderr).unwrap()
    }
}

const MANIFEST: &str = r#"
listen_port = 51820

[[node]]
name = "A"
address = "10.10.0.1/24"
public_key = "obuvsSP3vVFDjzrcwCWqgLmZeqEEVBGHIqzX3v4hYHA="
endpoint = "vpn-a.example.com:51820"
private_key = "wFW7oUjIpLCfZW2UwsfTlLDGrZb9iJH3bK6nosB5IGI="

[[node]]
name = "B"
address = "10.10.0.2/24"
public_key = "jEyKlv6hEMrKA5yzyFj6PYllHi2yNceWgXQ32HhuXCg="
endpoint = "vpn-b.example.com:51820"

[[node]]
name = "C"
address = "10.10.0.3/24"
public_key = "bnWs/u4aMMoN6C7/UkD/KdewhwBLFfcAsmroUlTVHnE="
endpoint = "vpn-c.example.com:51820"
"#;

#[test]
fn check_reports_ok_and_fails_on_a_broken_config() {
    let s = Sandbox::new();
    // A complete config with a known-good key pair -> ok, exit 0.
    let good = "[Interface]\n\
        PrivateKey = wFW7oUjIpLCfZW2UwsfTlLDGrZb9iJH3bK6nosB5IGI=\n\
        Address = 10.0.0.2/24\n\n\
        [Peer]\n\
        PublicKey = obuvsSP3vVFDjzrcwCWqgLmZeqEEVBGHIqzX3v4hYHA=\n\
        Endpoint = vpn.example:51820\n\
        AllowedIPs = 0.0.0.0/0\n";
    s.write("good.conf", good);
    let out = s.ok(&["check", "good.conf"]);
    assert!(out.contains("good.conf") && out.contains("ok"), "got: {out}");

    // Missing PrivateKey -> exit non-zero, reason on stderr.
    s.write("bad.conf", "[Interface]\nAddress = 10.0.0.2/24\n\n[Peer]\nPublicKey = obuvsSP3vVFDjzrcwCWqgLmZeqEEVBGHIqzX3v4hYHA=\n");
    let err = s.fails(&["check", "bad.conf"]);
    assert!(err.contains("PrivateKey"), "got: {err}");

    // A batch with one bad file still fails overall.
    s.fails(&["check", "good.conf", "bad.conf"]);
}

#[test]
fn key_prints_a_matching_pair() {
    let s = Sandbox::new();
    let out = s.ok(&["key"]);
    let private = out
        .lines()
        .find_map(|l| l.strip_prefix("PrivateKey = "))
        .unwrap()
        .to_string();
    let public = out
        .lines()
        .find_map(|l| l.strip_prefix("PublicKey  = "))
        .unwrap()
        .to_string();
    // deriving the pubkey from the printed private must reproduce it
    let derived = s.ok(&["pubkey", &private]);
    assert_eq!(derived.trim(), public);
}

#[test]
fn gen_writes_one_conf_per_node_minus_self() {
    let s = Sandbox::new();
    s.write("mesh.toml", MANIFEST);
    s.ok(&["mesh", "gen", "-m", "mesh.toml", "-o", "out"]);

    let a = std::fs::read_to_string(s.path("out/A.conf")).unwrap();
    assert!(a.contains("PrivateKey = wFW7oUjIpLCfZW2UwsfTlLDGrZb9iJH3bK6nosB5IGI="));
    // A peers with B and C, never itself
    assert!(a.contains("# B") && a.contains("# C"));
    assert!(!a.contains("obuvsSP3vVFDjzrcwCWqgLmZeqEEVBGHIqzX3v4hYHA="));
    assert!(a.contains("AllowedIPs = 10.10.0.2/32"));

    // B has no private key in the manifest -> placeholder, not a crash
    let b = std::fs::read_to_string(s.path("out/B.conf")).unwrap();
    assert!(b.contains("PrivateKey = <PASTE PRIVATE KEY>"));
    assert!(std::fs::read_to_string(s.path("out/C.conf")).is_ok());
}

#[test]
fn gen_on_missing_manifest_explains_itself() {
    let s = Sandbox::new();
    let err = s.fails(&["mesh", "gen", "-m", "nope.toml"]);
    assert!(err.contains("reading manifest"), "got: {err}");
}

#[test]
fn duplicate_address_manifest_is_rejected_with_reason() {
    let s = Sandbox::new();
    s.write(
        "dup.toml",
        r#"
[[node]]
name = "A"
address = "10.10.0.1/24"
public_key = "x"
[[node]]
name = "B"
address = "10.10.0.1/24"
public_key = "y"
"#,
    );
    let err = s.fails(&["mesh", "gen", "-m", "dup.toml"]);
    assert!(err.contains("duplicate mesh address"), "got: {err}");
}

#[test]
fn list_shows_all_confs_with_state() {
    let s = Sandbox::new();
    std::fs::write(s.path("wg0.conf"), "[Interface]\n").unwrap();
    std::fs::write(s.path("mesh.conf"), "[Interface]\n").unwrap();
    // readable dir -> no sudo; wg unavailable -> everything shows down
    let out = s.ok(&["list", "--dir", s.dir.path().to_str().unwrap()]);
    assert!(out.contains("wg0"), "got: {out}");
    assert!(out.contains("mesh"), "got: {out}");
    assert!(out.contains("down"), "got: {out}");
}

#[test]
fn status_shows_only_up_interfaces() {
    let s = Sandbox::new();
    std::fs::write(s.path("wg0.conf"), "[Interface]\n").unwrap();
    // wg unavailable in tests -> nothing is up
    let out = s.ok(&["status", "--dir", s.dir.path().to_str().unwrap()]);
    assert!(out.contains("nothing up"), "got: {out}");
    assert!(
        !out.contains("wg0"),
        "down interfaces must not appear in status"
    );
}

#[test]
fn dir_can_come_from_the_env() {
    let s = Sandbox::new();
    std::fs::write(s.path("wg7.conf"), "[Interface]\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ewg"))
        .args(["list"])
        .env("EWG_DIR", s.dir.path())
        .env("EWG_REGISTRY", s.path("registry.toml"))
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("wg7"));
}

#[test]
fn dir_add_list_rm_roundtrip() {
    let s = Sandbox::new();
    let d1 = s.path("a");
    let d2 = s.path("b");
    std::fs::create_dir_all(&d1).unwrap();
    std::fs::create_dir_all(&d2).unwrap();

    s.ok(&["dir", "add", d1.to_str().unwrap()]);
    s.ok(&["dir", "add", d2.to_str().unwrap()]);
    let listed = s.ok(&["dir", "list"]);
    assert!(
        listed.contains("/a") && listed.contains("/b"),
        "got: {listed}"
    );

    // json output is a parseable array
    let json = s.ok(&["dir", "list", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);

    s.ok(&["dir", "rm", d1.to_str().unwrap()]);
    let after = s.ok(&["dir", "list"]);
    assert!(
        !after.contains("/a") && after.contains("/b"),
        "got: {after}"
    );
}

#[test]
fn mesh_add_list_rm_gen_and_json() {
    let s = Sandbox::new();
    s.ok(&[
        "mesh",
        "add",
        "houseA",
        "--address",
        "10.10.0.1/24",
        "--pubkey",
        "PUBA",
        "--endpoint",
        "vpn-a:51820",
        "-m",
        "mesh.toml",
    ]);
    s.ok(&[
        "mesh",
        "add",
        "houseB",
        "--address",
        "10.10.0.2/24",
        "--pubkey",
        "PUBB",
        "-m",
        "mesh.toml",
    ]);

    // bare `mesh` lists names only (git-remote style)
    let list = s.ok(&["mesh", "-m", "mesh.toml"]);
    assert!(
        list.contains("houseA") && list.contains("houseB"),
        "got: {list}"
    );
    assert!(!list.contains("10.10.0.1"), "bare list is names only");
    // -v is verbose: names + ip + endpoint
    let verbose = s.ok(&["mesh", "-m", "mesh.toml", "-v"]);
    assert!(
        verbose.contains("houseA") && verbose.contains("10.10.0.1"),
        "got: {verbose}"
    );

    // json never leaks private keys and is valid
    let json = s.ok(&["mesh", "list", "-m", "mesh.toml", "--json"]);
    assert!(
        !json.contains("private"),
        "list json must not include private keys"
    );
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);

    // gen consumes the manifest we built
    s.ok(&["mesh", "gen", "-m", "mesh.toml", "-o", "out"]);
    let a = std::fs::read_to_string(s.path("out/houseA.conf")).unwrap();
    assert!(a.contains("# houseB") && a.contains("AllowedIPs = 10.10.0.2/32"));

    s.ok(&["mesh", "rm", "houseA", "-m", "mesh.toml"]);
    let after = s.ok(&["mesh", "list", "-m", "mesh.toml"]);
    assert!(
        !after.contains("houseA") && after.contains("houseB"),
        "got: {after}"
    );
}

#[test]
fn mesh_add_rejects_duplicate_address() {
    let s = Sandbox::new();
    s.ok(&[
        "mesh",
        "add",
        "A",
        "--address",
        "10.0.0.1/24",
        "--pubkey",
        "x",
        "-m",
        "m.toml",
    ]);
    let err = s.fails(&[
        "mesh",
        "add",
        "B",
        "--address",
        "10.0.0.1/24",
        "--pubkey",
        "y",
        "-m",
        "m.toml",
    ]);
    assert!(err.contains("already in use"), "got: {err}");
}

#[test]
fn ls_is_an_alias_for_list_everywhere() {
    let s = Sandbox::new();
    std::fs::write(s.path("wg0.conf"), "[Interface]\n").unwrap();
    // top-level: ewg ls == ewg list
    assert!(
        s.ok(&["ls", "--dir", s.dir.path().to_str().unwrap()])
            .contains("wg0")
    );
    // mesh ls == mesh list
    s.ok(&[
        "mesh",
        "add",
        "A",
        "--address",
        "10.9.0.1/24",
        "--pubkey",
        "x",
        "-m",
        "m.toml",
    ]);
    assert!(s.ok(&["mesh", "ls", "-m", "m.toml"]).contains("A"));
    // dir ls == dir list
    assert!(s.ok(&["dir", "ls"]).contains("wireguard"));
}

#[test]
fn bad_pubkey_input_is_a_clean_error_not_a_panic() {
    let s = Sandbox::new();
    let err = s.fails(&["pubkey", "!!!notbase64"]);
    assert!(err.contains("not valid base64"), "got: {err}");
}
