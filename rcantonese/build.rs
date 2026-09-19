fn main() {
        // Embed icons, version info and localized string tables into the DLL.
        embed_resource::compile("rcantonese.rc", embed_resource::NONE)
                .manifest_required()
                .unwrap();
        println!("cargo:rerun-if-changed=rcantonese.rc");
        println!("cargo:rerun-if-changed=resources");
}
