//! `rust-embed` ugrađuje popis datoteka **u vrijeme builda**, pa promjena web sučelja
//! (`web/dist`) mora pokrenuti ponovnu izgradnju — inače binarni fajl ostane bez novih
//! `assets/*.js` i sučelje se ne digne (vidi Fazu 4).

fn main() {
    println!("cargo:rerun-if-changed=../../web/dist");
    println!("cargo:rerun-if-changed=../../web/dist/index.html");
}
