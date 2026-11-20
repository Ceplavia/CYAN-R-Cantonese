fn main() {
        windows_reactor_setup::as_self_contained();
        // windows-reactor links TryCreatePackageDependency from
        // api-ms-win-appmodel-runtime-l1-1-5.dll. That contract starts at
        // Windows 11 (build 22000). Windows 10 21H2 (19044) and Windows
        // Server 2022 (20348) only have l1-1-3, so a hard import dies before
        // main. The self-contained runtime never calls it; delay-load keeps
        // 21H2 on the local WinUI binaries.
        println!("cargo:rustc-link-arg-bins=/DELAYLOAD:api-ms-win-appmodel-runtime-l1-1-5.dll");
        println!("cargo:rustc-link-lib=static=delayimp");
        // Embeds resources/config-center.rc (app icon) into the exe.
        embed_resource::compile("config-center.rc", embed_resource::NONE)
                .manifest_required()
                .unwrap();
        println!("cargo:rerun-if-changed=config-center.rc");
}
