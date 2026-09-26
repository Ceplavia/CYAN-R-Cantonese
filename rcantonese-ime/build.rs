fn main() {
        // .def keeps export names undecorated on x86 (stdcall symbols are
        // _Name@N by default, but imm32 resolves them via GetProcAddress
        // with the plain name).
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo:rustc-cdylib-link-arg=/DEF:{dir}\\exports.def");
        println!("cargo:rerun-if-changed=exports.def");
}
