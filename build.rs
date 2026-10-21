fn main() {
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/icon.ico");
        resource.set("ProductName", "LauncherCore");
        resource.set("FileDescription", "LauncherCore By HuanMeng");
        resource.set("OriginalFilename", "mclc.exe");
        resource
            .compile()
            .expect("failed to compile Windows resources");
    }
    println!("cargo:rerun-if-changed=assets/icon.ico");
}
