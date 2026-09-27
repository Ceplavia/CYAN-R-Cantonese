fn main() {
        // .def keeps export names undecorated on x86 (stdcall symbols are
        // _Name@N by default, but imm32 resolves them via GetProcAddress
        // with the plain name).
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo:rustc-cdylib-link-arg=/DEF:{dir}\\exports.def");
        println!("cargo:rerun-if-changed=exports.def");

        // ImmInstallIMEW requires a VERSIONINFO with FILETYPE=VFT_DRV and
        // FILESUBTYPE=VFT2_DRV_INPUTMETHOD — it fails with
        // ERROR_RESOURCE_TYPE_NOT_FOUND (1813) without it.
        embed_resource::compile("rcantonese-ime.rc", embed_resource::NONE)
                .manifest_required()
                .unwrap();
        println!("cargo:rerun-if-changed=rcantonese-ime.rc");
}
