use std::{env, path::PathBuf, process::Command};

fn output(program: &str, args: &[&str]) -> String {
    let result = Command::new(program)
        .args(args)
        .output()
        .expect("Run Xcode build tool");
    assert!(
        result.status.success(),
        "{program} failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout)
        .expect("UTF-8 Xcode tool path")
        .trim()
        .to_owned()
}

fn run(command: &mut Command) {
    let result = command.output().expect("Run native audio compiler");
    assert!(
        result.status.success(),
        "Native audio build failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn main() {
    println!("cargo:rerun-if-changed=native");
    println!("cargo:rerun-if-changed=assets/Info.plist");
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");
    println!("cargo:rerun-if-env-changed=SDKROOT");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    let arch = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86_64") => "x86_64",
        other => panic!("Unsupported macOS audio architecture: {other:?}"),
    };
    let sdk = output("xcrun", &["--sdk", "macosx", "--show-sdk-path"]);
    let swiftc = PathBuf::from(output("xcrun", &["--find", "swiftc"]));
    let swift_lib = swiftc
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("lib/swift/macosx");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let c_object = out.join("PCMQueue.o");
    let swift_object = out.join("MeetingAudio.o");
    run(Command::new("xcrun")
        .args([
            "clang",
            "-std=c11",
            "-O2",
            "-arch",
            arch,
            "-isysroot",
            &sdk,
            "-mmacosx-version-min=14.2",
            "-c",
            "native/PCMQueue.c",
            "-o",
        ])
        .arg(&c_object));
    run(Command::new(&swiftc)
        .args([
            "-swift-version",
            "6",
            "-O",
            "-parse-as-library",
            "-whole-module-optimization",
            "-module-name",
            "MeetingAudio",
            "-target",
            &format!("{arch}-apple-macosx14.2"),
            "-sdk",
            &sdk,
            "-I",
            "native",
            "-emit-object",
            "native/MeetingAudio.swift",
            "native/AudioWatchdog.swift",
            "native/MenuBar.swift",
            "-o",
        ])
        .arg(&swift_object));
    run(Command::new("xcrun")
        .args(["libtool", "-static", "-o"])
        .arg(out.join("libmeeting_audio.a"))
        .arg(&c_object)
        .arg(&swift_object));

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-search=native={}", swift_lib.display());
    println!("cargo:rustc-link-search=native={sdk}/usr/lib/swift");
    println!("cargo:rustc-link-lib=static=meeting_audio");
    for framework in [
        "AVFoundation",
        "AppKit",
        "AudioToolbox",
        "CoreAudio",
        "Foundation",
        "ServiceManagement",
        "UserNotifications",
    ] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    println!("cargo:rustc-link-arg=-mmacosx-version-min=14.2");
    // `cargo run` also needs usage descriptions; macOS terminates a process
    // that asks for microphone access without an Info.plist purpose string.
    let info = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("assets/Info.plist");
    println!(
        "cargo:rustc-link-arg=-Wl,-sectcreate,__TEXT,__info_plist,{}",
        info.display()
    );
}
