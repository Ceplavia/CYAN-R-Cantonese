fn main() {
        // Embed the mode icons — same .ico files the DLL uses.
        embed_resource::compile("tray.rc", embed_resource::NONE)
                .manifest_required()
                .unwrap();
        println!("cargo:rerun-if-changed=tray.rc");
}
