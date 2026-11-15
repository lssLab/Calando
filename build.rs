//! Windows: the executable carries the Calando icon and the name Task Manager shows.

fn main() {
    println!("cargo:rerun-if-changed=packaging/windows/calando.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("packaging/windows/calando.ico")
            .set("FileDescription", "Calando")
            .set("ProductName", "Calando");
        resource.compile().expect("Windows resources must compile");
    }
}
