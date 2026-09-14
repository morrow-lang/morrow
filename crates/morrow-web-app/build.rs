//! Compile the shared Morrow application's server entry at build time.
use ar_archive_writer::{
    ArchiveKind, DEFAULT_OBJECT_READER, NewArchiveMember, write_archive_to_stream,
};
use morrow_compiler::{
    check, cranelift, modules,
    native_library::{self, Export},
};
use std::{env, fs, io, path::PathBuf};

fn main() -> io::Result<()> {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../examples/web/server.fn");
    let source = modules::load(&root).map_err(|e| io::Error::other(e.message))?;
    for file in source.sources() {
        println!("cargo:rerun-if-changed={}", file.path.display());
    }
    let checked =
        check::check_library(&source.program).map_err(|e| io::Error::other(source.render(e)))?;
    let program = native_library::lower(
        &checked,
        &[
            Export::new("server.start_room", "start_room"),
            Export::new("server.send_command", "send_command"),
            Export::new("server.inspect_room", "inspect_room"),
            Export::new("server.restore_room", "restore_room"),
        ],
    )
    .map_err(|e| io::Error::other(source.render(e)))?;
    let target = env::var("TARGET").map_err(io::Error::other)?;
    let bytes = cranelift::emit_object_for_target(&program, &target).map_err(io::Error::other)?;
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    // Archive format follows the compilation target, not the build host's `ar`.
    // Recreate this build's archive so removed exports cannot survive rebuilds.
    let kind = if target.ends_with("-apple-darwin") {
        ArchiveKind::Darwin
    } else {
        ArchiveKind::Gnu
    };
    let members = [NewArchiveMember::new(
        bytes,
        &DEFAULT_OBJECT_READER,
        "morrow_application.o".into(),
    )];
    let mut archive = fs::File::create(output.join("libmorrow_application.a"))?;
    write_archive_to_stream(&mut archive, &members, kind, false, None)?;
    println!("cargo:rustc-link-search=native={}", output.display());
    println!("cargo:rustc-link-lib=static=morrow_application");
    Ok(())
}
