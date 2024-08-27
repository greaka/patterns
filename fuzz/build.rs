fn main() {
    println!("cargo::rerun-if-changed=corpus/diff");

    use std::{io::Write, path::PathBuf};

    let files = std::fs::read_dir("corpus/diff").unwrap().flatten();

    let out_dir = dbg!(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut out_file =
        std::fs::File::create(PathBuf::from(out_dir).join("fuzz_targets/data.rs")).unwrap();

    writeln!(&mut out_file, "pub static DATA: &[&[u8]] = &[").unwrap();
    for entry in files {
        let content = std::fs::read(entry.path()).unwrap();
        writeln!(
            &mut out_file,
            "&[{}],",
            content
                .iter()
                .map(|ch| ch.to_string())
                .collect::<Vec<_>>()
                .join(",")
        )
        .unwrap();
    }
    writeln!(&mut out_file, "];").unwrap();
}
