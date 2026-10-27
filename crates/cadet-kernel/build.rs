/// OCCT toolkits we link, dependents first.
const LIBS: &[&str] = &[
    "TKDESTEP",
    "TKDEIGES",
    "TKDESTL",
    "TKXSBase",
    "TKDE",
    "TKFillet",
    "TKBool",
    "TKBO",
    "TKMesh",
    "TKShHealing",
    "TKPrim",
    "TKTopAlgo",
    "TKGeomAlgo",
    "TKBRep",
    "TKGeomBase",
    "TKG3d",
    "TKG2d",
    "TKMath",
    "TKernel",
];

fn main() {
    occt_sys::build_occt();
    let occt = occt_sys::occt_path();
    cxx_build::bridge("src/ffi.rs").file("src/occt.cpp").include(occt.join("include")).std("c++17").flag_if_supported("-w").compile("cadet_occt");
    println!("cargo:rustc-link-search=native={}", occt.join("lib").display());
    for lib in LIBS {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // OCCT's OSD layer uses registry, security and user-name APIs.
        for lib in ["user32", "advapi32"] {
            println!("cargo:rustc-link-lib=dylib={lib}");
        }
    }
    for f in ["src/ffi.rs", "src/occt.cpp", "src/occt.h"] {
        println!("cargo:rerun-if-changed={f}");
    }
}
