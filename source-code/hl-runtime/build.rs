fn main() {
    cc::Build::new()
    .file("src/gc.c")
    .compile("gc");
}
