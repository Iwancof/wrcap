fn main() {
    use cc::Build;

    Build::new()
        .file("./src/fops.c")
        .compile("low");

    println!("cargo:rerun-if-changed=src/fops.c");
}
