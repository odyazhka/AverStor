fn main() {
    cc::Build::new()
        .file("src/pty_runner.c")
        .flag("-O2")
        .flag("-Wall")
        .compile("pty_runner");

    println!("cargo:rerun-if-changed=src/pty_runner.c");
}
