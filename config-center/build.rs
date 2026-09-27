fn main() {
        windows_reactor_setup::as_self_contained();
        // Embeds resources/config-center.rc (app icon) into the exe.
        embed_resource::compile("config-center.rc", embed_resource::NONE)
                .manifest_required()
                .unwrap();
        println!("cargo:rerun-if-changed=config-center.rc");
}
