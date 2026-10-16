//! Rebuilds the app when translations change: they are embedded at compile
//! time, but nothing else tells cargo that the binary depends on them.

fn main() {
    println!("cargo:rerun-if-changed=locales");
}
