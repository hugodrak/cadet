# Third-party software in Cadet builds

- **Open CASCADE Technology 7.8** (geometry kernel): LGPL-2.1 with the OCCT exception. https://github.com/Open-Cascade-SAS/OCCT
  Statically linked. Cadet's complete source is public, so you can rebuild Cadet against a modified OCCT (see README → Build from source).
- **Inter** font: SIL Open Font License 1.1. https://github.com/rsms/inter
- **Phosphor Icons**: MIT. https://github.com/phosphor-icons/web
- Rust crates (egui, wgpu, glam, serde, rfd, cxx, occt-sys, …): MIT and/or Apache-2.0. `cargo tree` lists them all.
